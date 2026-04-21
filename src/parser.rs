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
    /// User-declared sub names (for allowing UDF to shadow stryke extensions in compat mode).
    declared_subs: std::collections::HashSet<String>,
    /// When > 0, `parse_named_expr` will not consume following barewords as paren-less
    /// function arguments. Used by thread macro to prevent `t Color::Red p` from
    /// interpreting `p` as an argument to the enum constructor instead of a stage.
    suppress_parenless_call: u32,
    /// When > 0, `parse_multiplication` will not consume `Token::Slash` as division.
    /// Used by thread macro so `/pattern/` is left for the stage parser to handle.
    suppress_slash_as_div: u32,
    /// When > 0, the lexer should not interpret `m/`, `s/`, etc. as regex-starters.
    /// Used by thread macro to prevent `/m/` from being misparsed.
    pub suppress_m_regex: u32,
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
            declared_subs: std::collections::HashSet::new(),
            suppress_parenless_call: 0,
            suppress_slash_as_div: 0,
            suppress_m_regex: 0,
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
    /// stryke extension contexts (map/grep/fore expression forms, pipe-forward)
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
            // rev with empty list should use $_
            ExprKind::Rev(ref inner) => {
                if matches!(inner.kind, ExprKind::List(ref v) if v.is_empty()) {
                    Expr {
                        kind: ExprKind::Rev(Box::new(topic())),
                        line,
                    }
                } else {
                    expr
                }
            }
            _ => expr,
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

    /// Line number of the most recently consumed token (the token at `pos - 1`).
    fn prev_line(&self) -> usize {
        if self.pos > 0 {
            self.tokens.get(self.pos - 1).map(|(_, l)| *l).unwrap_or(0)
        } else {
            0
        }
    }

    /// Check if `{ ... }` starting at current position looks like a hashref rather than a block.
    /// Heuristics (assuming current token is `{`):
    /// - `{ bareword =>` → hashref
    /// - `{ "string" =>` → hashref
    /// - `{ $var =>` → hashref
    /// - `{ 0 =>` → hashref (numeric key)
    /// - `{ %hash }` or `{ %hash, ...}` → hashref (spread)
    /// - `{ }` (empty) → hashref
    fn looks_like_hashref(&self) -> bool {
        debug_assert!(matches!(self.peek(), Token::LBrace));
        let tok1 = self.peek_at(1);
        let tok2 = self.peek_at(2);
        match tok1 {
            Token::RBrace => true,
            Token::Ident(_)
            | Token::SingleString(_)
            | Token::DoubleString(_)
            | Token::ScalarVar(_)
            | Token::Integer(_) => matches!(tok2, Token::FatArrow),
            Token::HashVar(_) => matches!(tok2, Token::RBrace | Token::Comma),
            _ => false,
        }
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

    /// True when the next token is a statement-starting keyword on a *different*
    /// line from `stmt_line`.  Used by `parse_use` / `parse_no` to stop parsing
    /// import lists when semicolons are omitted (stryke extension).
    fn next_is_new_stmt_keyword(&self, stmt_line: usize) -> bool {
        // Semicolons-optional is a stryke extension; in compat mode, require them.
        if crate::compat_mode() {
            return false;
        }
        if self.peek_line() == stmt_line {
            return false;
        }
        matches!(
            self.peek(),
            Token::Ident(ref kw) if matches!(kw.as_str(),
                "use" | "no" | "my" | "our" | "local" | "sub" | "struct" | "enum"
                | "if" | "unless" | "while" | "until" | "for" | "foreach"
                | "return" | "last" | "next" | "redo" | "package" | "require"
                | "BEGIN" | "END" | "UNITCHECK" | "frozen" | "const" | "typed"
            )
        )
    }

    /// True when the next token is on a different line from `stmt_line` and could
    /// start a new statement. More permissive than `next_is_new_stmt_keyword` —
    /// includes sigil-prefixed variables like `$var`, `@arr`, `%hash`.
    fn next_is_new_statement_start(&self, stmt_line: usize) -> bool {
        if crate::compat_mode() {
            return false;
        }
        if self.peek_line() == stmt_line {
            return false;
        }
        matches!(
            self.peek(),
            Token::ScalarVar(_)
                | Token::DerefScalarVar(_)
                | Token::ArrayVar(_)
                | Token::HashVar(_)
                | Token::LBrace
        ) || self.next_is_new_stmt_keyword(stmt_line)
    }

    // ── Top level ──

    pub fn parse_program(&mut self) -> PerlResult<Program> {
        let statements = self.parse_statements()?;
        Ok(Program { statements })
    }

    /// Parse statements until EOF. Used by parse_program and parse_block_from_str.
    pub fn parse_statements(&mut self) -> PerlResult<Vec<Statement>> {
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
        Ok(statements)
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
                "sub" => self.parse_sub_decl(true)?,
                "fn" => self.parse_sub_decl(false)?,
                "struct" => {
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`struct` is a stryke extension (disabled by --compat)",
                            self.peek_line(),
                        ));
                    }
                    self.parse_struct_decl()?
                }
                "enum" => {
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`enum` is a stryke extension (disabled by --compat)",
                            self.peek_line(),
                        ));
                    }
                    self.parse_enum_decl()?
                }
                "class" => {
                    if crate::compat_mode() {
                        // TODO: parse Perl 5.38 class syntax with :isa()
                        return Err(self.syntax_err(
                            "Perl 5.38 `class` syntax not yet implemented in --compat mode",
                            self.peek_line(),
                        ));
                    }
                    self.parse_class_decl(false, false)?
                }
                "abstract" => {
                    self.advance(); // abstract
                    if !matches!(self.peek(), Token::Ident(ref s) if s == "class") {
                        return Err(self.syntax_err(
                            "`abstract` must be followed by `class`",
                            self.peek_line(),
                        ));
                    }
                    self.parse_class_decl(true, false)?
                }
                "final" => {
                    self.advance(); // final
                    if !matches!(self.peek(), Token::Ident(ref s) if s == "class") {
                        return Err(self
                            .syntax_err("`final` must be followed by `class`", self.peek_line()));
                    }
                    self.parse_class_decl(false, true)?
                }
                "trait" => {
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`trait` is a stryke extension (disabled by --compat)",
                            self.peek_line(),
                        ));
                    }
                    self.parse_trait_decl()?
                }
                "my" => self.parse_my_our_local("my", false)?,
                "state" => self.parse_my_our_local("state", false)?,
                "mysync" => {
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`mysync` is a stryke extension (disabled by --compat)",
                            self.peek_line(),
                        ));
                    }
                    self.parse_my_our_local("mysync", false)?
                }
                "frozen" | "const" => {
                    let leading = kw.as_str().to_string();
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            format!("`{leading}` is a stryke extension (disabled by --compat)"),
                            self.peek_line(),
                        ));
                    }
                    // `frozen my $x = val;` / `const my $x = val;` — the
                    // two spellings are interchangeable (`const` is the
                    // more-familiar name for new users). Expects `my`
                    // to follow.
                    self.advance(); // consume "frozen"/"const"
                    if let Token::Ident(ref kw) = self.peek().clone() {
                        if kw == "my" {
                            let mut stmt = self.parse_my_our_local("my", false)?;
                            if let StmtKind::My(ref mut decls) = stmt.kind {
                                for decl in decls.iter_mut() {
                                    decl.frozen = true;
                                }
                            }
                            stmt
                        } else {
                            return Err(self.syntax_err(
                                format!("Expected 'my' after '{leading}'"),
                                self.peek_line(),
                            ));
                        }
                    } else {
                        return Err(self.syntax_err(
                            format!("Expected 'my' after '{leading}'"),
                            self.peek_line(),
                        ));
                    }
                }
                "typed" => {
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`typed` is a stryke extension (disabled by --compat)",
                            self.peek_line(),
                        ));
                    }
                    self.advance();
                    if let Token::Ident(ref kw) = self.peek().clone() {
                        if kw == "my" {
                            self.parse_my_our_local("my", true)?
                        } else {
                            return Err(
                                self.syntax_err("Expected 'my' after 'typed'", self.peek_line())
                            );
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
                "defer" => self.parse_defer_stmt()?,
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
                // Disambiguate hashref `{ k => v }` from block `{ stmt; stmt }`.
                // If it looks like a hashref, parse as expression; otherwise parse as block.
                if self.looks_like_hashref() {
                    let expr = self.parse_expression()?;
                    let stmt = self.maybe_postfix_modifier(expr)?;
                    self.parse_stmt_postfix_modifier(stmt)?
                } else {
                    let block = self.parse_block()?;
                    let stmt = Statement {
                        label: None,
                        kind: StmtKind::Block(block),
                        line,
                    };
                    // `{ … } if EXPR` / `{ … } unless EXPR` — same postfix rule as `do { } if …` (not `if (`).
                    self.parse_stmt_postfix_modifier(stmt)?
                }
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
        // Implicit semicolon: a modifier keyword on a new line is a new
        // statement, not a postfix modifier.  This prevents semicolon-less
        // code like `my $x = "val"\nif ($x) { ... }` from being mis-parsed
        // as `my $x = "val" if ($x) { ... }`.
        if self.peek_line() > self.prev_line() {
            self.eat(&Token::Semicolon);
            return Ok(stmt);
        }
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
                            stream: false,
                        },
                        "pflat_map" => ExprKind::PMapExpr {
                            block,
                            list,
                            progress,
                            flat_outputs: true,
                            on_cluster: None,
                            stream: false,
                        },
                        "pgrep" => ExprKind::PGrepExpr {
                            block,
                            list,
                            progress,
                            stream: false,
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
        // Implicit semicolon: modifier keyword on a new line starts a new statement.
        if self.peek_line() > self.prev_line() {
            return Ok(Statement {
                label: None,
                kind: StmtKind::Expression(expr),
                line,
            });
        }
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
                | "cat"
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
                | "dec"
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
                | "d"
                | "dirs"
                | "dr"
                | "f"
                | "fi"
                | "files"
                | "filesf"
                | "filter"
                | "fr"
                | "getcwd"
                | "glob_par"
                | "par_sed"
                | "glob"
                | "grep"
                | "greps"
                | "heap"
                | "hex"
                | "inc"
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
                | "maps"
                | "flat_maps"
                | "flatten"
                | "frequencies"
                | "freq"
                | "interleave"
                | "ddump"
                | "stringify"
                | "str"
                | "s"
                | "input"
                | "lines"
                | "words"
                | "chars"
                | "digits"
                | "letters"
                | "letters_uc"
                | "letters_lc"
                | "punctuation"
                | "sentences"
                | "paragraphs"
                | "sections"
                | "numbers"
                | "graphemes"
                | "columns"
                | "trim"
                | "avg"
                | "top"
                | "pager"
                | "pg"
                | "less"
                | "count_by"
                | "to_file"
                | "to_json"
                | "to_csv"
                | "grep_v"
                | "select_keys"
                | "pluck"
                | "clamp"
                | "normalize"
                | "stddev"
                | "squared"
                | "square"
                | "cubed"
                | "cube"
                | "expt"
                | "pow"
                | "pw"
                | "snake_case"
                | "camel_case"
                | "kebab_case"
                | "to_toml"
                | "to_yaml"
                | "to_xml"
                | "to_html"
                | "to_markdown"
                | "xopen"
                | "clip"
                | "paste"
                | "to_table"
                | "sparkline"
                | "bar_chart"
                | "flame"
                | "set"
                | "list_count"
                | "list_size"
                | "count"
                | "size"
                | "cnt"
                | "len"
                | "all"
                | "any"
                | "none"
                | "take_while"
                | "drop_while"
                | "skip_while"
                | "skip"
                | "first_or"
                | "tap"
                | "peek"
                | "partition"
                | "min_by"
                | "max_by"
                | "zip_with"
                | "group_by"
                | "chunk_by"
                | "with_index"
                | "puniq"
                | "pfirst"
                | "pany"
                | "uniq"
                | "distinct"
                | "shuffle"
                | "shuffled"
                | "chunked"
                | "windowed"
                | "match"
                | "mkdir"
                | "every"
                | "gen"
                | "oct"
                | "open"
                | "p"
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
                | "pgreps"
                | "pipeline"
                | "pmap_chunked"
                | "pmap_reduce"
                | "pmap_on"
                | "pflat_map_on"
                | "pmap"
                | "pmaps"
                | "pflat_map"
                | "pflat_maps"
                | "pop"
                | "pos"
                | "ppool"
                | "preduce_init"
                | "preduce"
                | "pselect"
                | "printf"
                | "print"
                | "pr"
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
                | "rev"
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
        // `{ |$a, $b| body }` — Ruby-style block params.
        // Desugars to `my $a = $_` (1 param), `my $a = $a; my $b = $b` (2 — sort/reduce),
        // or `my $p = $_N` for positional N≥3.
        if let Some(param_stmts) = self.try_parse_block_params()? {
            stmts.extend(param_stmts);
        }
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            if self.eat(&Token::Semicolon) {
                continue;
            }
            stmts.push(self.parse_statement()?);
        }
        self.expect(&Token::RBrace)?;
        Self::default_topic_for_sole_bareword(&mut stmts);
        Ok(stmts)
    }

    /// Try to parse `|$var1, $var2, ...|` at the start of a block.
    /// Returns `None` if the leading `|` is not block-param syntax.
    /// When successful, returns `my $var = <implicit>` assignment statements
    /// that alias the block's positional arguments.
    fn try_parse_block_params(&mut self) -> PerlResult<Option<Vec<Statement>>> {
        if !matches!(self.peek(), Token::BitOr) {
            return Ok(None);
        }
        // Lookahead: `| $scalar [, $scalar]* |` — verify before consuming.
        let mut i = 1; // skip the opening `|`
        loop {
            match self.peek_at(i) {
                Token::ScalarVar(_) => i += 1,
                _ => return Ok(None), // not `|$var...|`
            }
            match self.peek_at(i) {
                Token::BitOr => break,  // closing `|`
                Token::Comma => i += 1, // more params
                _ => return Ok(None),   // not block params
            }
        }
        // Confirmed — consume and build assignments.
        let line = self.peek_line();
        self.advance(); // eat opening `|`
        let mut names = Vec::new();
        loop {
            if let Token::ScalarVar(ref name) = self.peek().clone() {
                names.push(name.clone());
                self.advance();
            }
            if self.eat(&Token::BitOr) {
                break;
            }
            self.expect(&Token::Comma)?;
        }
        // Generate `my $name = <source>` for each param.
        // 1 param  → source is `$_` (map/grep/each/for topic)
        // 2 params → sources are `$a`, `$b` (sort/reduce)
        // N params → sources are `$_`, `$_1`, `$_2`, … (positional)
        let sources: Vec<&str> = match names.len() {
            1 => vec!["_"],
            2 => vec!["a", "b"],
            n => {
                // Can't return borrowed from a generated vec, handle below.
                let _ = n;
                vec![] // sentinel — handled in the else branch
            }
        };
        let mut stmts = Vec::with_capacity(names.len());
        if !sources.is_empty() {
            for (name, src) in names.iter().zip(sources.iter()) {
                stmts.push(Statement {
                    label: None,
                    kind: StmtKind::My(vec![VarDecl {
                        sigil: Sigil::Scalar,
                        name: name.clone(),
                        initializer: Some(Expr {
                            kind: ExprKind::ScalarVar(src.to_string()),
                            line,
                        }),
                        frozen: false,
                        type_annotation: None,
                    }]),
                    line,
                });
            }
        } else {
            // N≥3: positional `$_`, `$_1`, `$_2`, …
            for (idx, name) in names.iter().enumerate() {
                let src = if idx == 0 {
                    "_".to_string()
                } else {
                    format!("_{idx}")
                };
                stmts.push(Statement {
                    label: None,
                    kind: StmtKind::My(vec![VarDecl {
                        sigil: Sigil::Scalar,
                        name: name.clone(),
                        initializer: Some(Expr {
                            kind: ExprKind::ScalarVar(src),
                            line,
                        }),
                        frozen: false,
                        type_annotation: None,
                    }]),
                    line,
                });
            }
        }
        Ok(Some(stmts))
    }

    /// Block shorthand: when the body is literally one bare builtin call
    /// (`{ uc }`, `{ basename }`, `{ to_json }`), inject `$_` as its first
    /// argument so `map { basename }` == `map { basename($_) }` uniformly.
    ///
    /// Without this, the ExprKind-modeled core names (`uc`/`lc`/`length`/…)
    /// default to `$_` via their own parse arms, but generic `FuncCall`-
    /// dispatched builtins (`basename`/`to_json`/`tj`/`bn`) are called with
    /// empty args and return the wrong value. This rewrite levels the
    /// playing field at parse time — no per-builtin handling needed.
    ///
    /// Narrow by design: fires only when the block has *exactly one*
    /// expression statement whose sole content is a known-bareword call
    /// with zero args. Multi-statement blocks and blocks with any other
    /// content are untouched.
    fn default_topic_for_sole_bareword(stmts: &mut [Statement]) {
        let [only] = stmts else { return };
        let StmtKind::Expression(ref mut expr) = only.kind else {
            return;
        };
        let topic_line = expr.line;
        let topic_arg = || Expr {
            kind: ExprKind::ScalarVar("_".to_string()),
            line: topic_line,
        };
        match expr.kind {
            // Zero-arg FuncCall whose name is a known builtin → inject `$_`.
            ExprKind::FuncCall {
                ref name,
                ref mut args,
            } if args.is_empty()
                && (Self::is_known_bareword(name) || Self::is_try_builtin_name(name)) =>
            {
                args.push(topic_arg());
            }
            // Lone bareword (the parser sometimes keeps a bareword as a
            // `Bareword` node instead of a zero-arg `FuncCall` —
            // e.g. `{ to_json }`, `{ ddump }`). Promote to a call.
            ExprKind::Bareword(ref name)
                if (Self::is_known_bareword(name) || Self::is_try_builtin_name(name)) =>
            {
                let n = name.clone();
                expr.kind = ExprKind::FuncCall {
                    name: n,
                    args: vec![topic_arg()],
                };
            }
            _ => {}
        }
    }

    /// `defer { BLOCK }` — register a block to run when the current scope exits.
    /// Desugars to a `defer__internal(sub { BLOCK })` function call that the compiler
    /// handles specially by emitting Op::DeferBlock.
    fn parse_defer_stmt(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // defer
        let body = self.parse_block()?;
        self.eat(&Token::Semicolon);
        // Desugar: defer { BLOCK } → defer__internal(fn { BLOCK })
        let coderef = Expr {
            kind: ExprKind::CodeRef {
                params: vec![],
                body,
            },
            line,
        };
        Ok(Statement {
            label: None,
            kind: StmtKind::Expression(Expr {
                kind: ExprKind::FuncCall {
                    name: "defer__internal".to_string(),
                    args: vec![coderef],
                },
                line,
            }),
            line,
        })
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

    /// `thread EXPR stage1 stage2 ...` — Clojure-style threading macro.
    /// Desugars to `EXPR |> stage1 |> stage2 |> ...`
    ///
    /// When invoked as the RHS of `|>` (e.g. `LHS |> t s1 s2 ...`), the init
    /// is not parsed from tokens — using `parse_unary()` there lets the first
    /// bareword greedily consume the next token as its arg, which misparses
    /// `t inc pow($_, 2) p` as init=`inc(pow(…))` + stage=`p` instead of three
    /// separate stages. Instead, seed init with `$_[0]`, run every remaining
    /// token through the stage loop, and wrap the resulting chain in a
    /// `CodeRef`. The outer `pipe_forward_apply` then calls it with `lhs` as
    /// `$_[0]`, giving `LHS |> t s1 s2 s3` == `LHS |> s1 |> s2 |> s3`.
    fn parse_thread_macro(&mut self, _line: usize) -> PerlResult<Expr> {
        let pipe_rhs_wrap = self.in_pipe_rhs();
        let mut result = if pipe_rhs_wrap {
            Expr {
                kind: ExprKind::ArrayElement {
                    array: "_".to_string(),
                    index: Box::new(Expr {
                        kind: ExprKind::Integer(0),
                        line: _line,
                    }),
                },
                line: _line,
            }
        } else {
            // Suppress paren-less function calls so `t Color::Red p` parses
            // the enum variant without consuming `p` as an argument.
            self.suppress_parenless_call = self.suppress_parenless_call.saturating_add(1);
            let expr = self.parse_thread_input();
            self.suppress_parenless_call = self.suppress_parenless_call.saturating_sub(1);
            expr?
        };

        // Track line where the last stage ended (initially the input expression's line).
        let mut last_stage_end_line = self.prev_line();

        // Parse stages until we hit a statement terminator
        loop {
            // Newline termination: if the next token is on a different line than where
            // the previous stage ended, the thread macro terminates. This allows
            // `~> @arr map { $_ * 2 }` on one line followed by `my @b = ...` on the next
            // without requiring a semicolon.
            if self.peek_line() > last_stage_end_line {
                break;
            }

            // Check for terminators - |> ends thread and allows piping the result.
            // Variables ($x, @x, %x) and declaration keywords (my, our, local, state)
            // cannot be stages, so they implicitly terminate the thread macro.
            match self.peek() {
                Token::Semicolon
                | Token::RBrace
                | Token::RParen
                | Token::RBracket
                | Token::PipeForward
                | Token::Eof
                | Token::ScalarVar(_)
                | Token::ArrayVar(_)
                | Token::HashVar(_)
                | Token::Comma => break,
                Token::Ident(ref kw)
                    if matches!(
                        kw.as_str(),
                        "my" | "our"
                            | "local"
                            | "state"
                            | "if"
                            | "unless"
                            | "while"
                            | "until"
                            | "for"
                            | "foreach"
                            | "return"
                            | "last"
                            | "next"
                            | "redo"
                    ) =>
                {
                    break
                }
                _ => {}
            }

            let stage_line = self.peek_line();

            // Parse a stage and apply it to result via pipe
            match self.peek().clone() {
                // `>{ block }` — standalone anonymous block (sugar for fn { })
                Token::ArrowBrace => {
                    self.advance(); // consume `>{`
                    let mut stmts = Vec::new();
                    while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                        if self.eat(&Token::Semicolon) {
                            continue;
                        }
                        stmts.push(self.parse_statement()?);
                    }
                    self.expect(&Token::RBrace)?;
                    let code_ref = Expr {
                        kind: ExprKind::CodeRef {
                            params: vec![],
                            body: stmts,
                        },
                        line: stage_line,
                    };
                    result = self.pipe_forward_apply(result, code_ref, stage_line)?;
                }
                // `fn { block }` — only valid in compat mode
                Token::Ident(ref name) if name == "sub" => {
                    if !crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`fn {}` anonymous subroutine is not valid stryke; use `fn {}` instead",
                            stage_line,
                        ));
                    }
                    self.advance(); // consume `sub`
                    let (params, _prototype) = self.parse_sub_sig_or_prototype_opt()?;
                    let body = self.parse_block()?;
                    let code_ref = Expr {
                        kind: ExprKind::CodeRef { params, body },
                        line: stage_line,
                    };
                    result = self.pipe_forward_apply(result, code_ref, stage_line)?;
                }
                // `fn { block }` — stryke anonymous function
                Token::Ident(ref name) if name == "fn" => {
                    self.advance(); // consume `fn`
                    let (params, _prototype) = self.parse_sub_sig_or_prototype_opt()?;
                    let body = self.parse_block()?;
                    let code_ref = Expr {
                        kind: ExprKind::CodeRef { params, body },
                        line: stage_line,
                    };
                    result = self.pipe_forward_apply(result, code_ref, stage_line)?;
                }
                // `ident` possibly followed by block
                Token::Ident(ref name) => {
                    let func_name = name.clone();
                    self.advance();

                    // Handle s/// and tr/// encoded tokens
                    if func_name.starts_with('\x00') {
                        let parts: Vec<&str> = func_name.split('\x00').collect();
                        if parts.len() >= 4 && parts[1] == "s" {
                            let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                            let stage = Expr {
                                kind: ExprKind::Substitution {
                                    expr: Box::new(result.clone()),
                                    pattern: parts[2].to_string(),
                                    replacement: parts[3].to_string(),
                                    flags: format!("{}r", parts.get(4).unwrap_or(&"")),
                                    delim,
                                },
                                line: stage_line,
                            };
                            result = stage;
                            last_stage_end_line = self.prev_line();
                            continue;
                        }
                        if parts.len() >= 4 && parts[1] == "tr" {
                            let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                            let stage = Expr {
                                kind: ExprKind::Transliterate {
                                    expr: Box::new(result.clone()),
                                    from: parts[2].to_string(),
                                    to: parts[3].to_string(),
                                    flags: format!("{}r", parts.get(4).unwrap_or(&"")),
                                    delim,
                                },
                                line: stage_line,
                            };
                            result = stage;
                            last_stage_end_line = self.prev_line();
                            continue;
                        }
                        return Err(
                            self.syntax_err("Unexpected encoded token in thread", stage_line)
                        );
                    }

                    // `map +{ ... }` — hashref expression form (not a code block).
                    // The `+` disambiguates: `+{` is always a hashref constructor.
                    // Desugars to `MapExprComma` so pipe_forward_apply threads the
                    // list correctly: `t LIST map +{k => $_}` → `map +{k => $_}, LIST`.
                    if matches!(self.peek(), Token::Plus)
                        && matches!(self.peek_at(1), Token::LBrace)
                    {
                        self.advance(); // consume `+`
                        self.expect(&Token::LBrace)?;
                        // try_parse_hash_ref consumes the closing `}`
                        let pairs = self.try_parse_hash_ref()?;
                        let hashref_expr = Expr {
                            kind: ExprKind::HashRef(pairs),
                            line: stage_line,
                        };
                        let flatten_array_refs =
                            matches!(func_name.as_str(), "flat_map" | "flat_maps");
                        let stream = matches!(func_name.as_str(), "maps" | "flat_maps");
                        // Placeholder list — pipe_forward_apply replaces it with `result`.
                        let placeholder = Expr {
                            kind: ExprKind::Undef,
                            line: stage_line,
                        };
                        let map_node = Expr {
                            kind: ExprKind::MapExprComma {
                                expr: Box::new(hashref_expr),
                                list: Box::new(placeholder),
                                flatten_array_refs,
                                stream,
                            },
                            line: stage_line,
                        };
                        result = self.pipe_forward_apply(result, map_node, stage_line)?;
                    // `pmap_chunked CHUNK_SIZE { BLOCK }` — parallel chunked map
                    } else if func_name == "pmap_chunked" {
                        let chunk_size = self.parse_assign_expr()?;
                        let block = self.parse_block_or_bareword_block()?;
                        let placeholder = self.pipe_placeholder_list(stage_line);
                        let stage = Expr {
                            kind: ExprKind::PMapChunkedExpr {
                                chunk_size: Box::new(chunk_size),
                                block,
                                list: Box::new(placeholder),
                                progress: None,
                            },
                            line: stage_line,
                        };
                        result = self.pipe_forward_apply(result, stage, stage_line)?;
                    // `preduce_init INIT { BLOCK }` — parallel reduce with init value
                    } else if func_name == "preduce_init" {
                        let init = self.parse_assign_expr()?;
                        let block = self.parse_block_or_bareword_block()?;
                        let placeholder = self.pipe_placeholder_list(stage_line);
                        let stage = Expr {
                            kind: ExprKind::PReduceInitExpr {
                                init: Box::new(init),
                                block,
                                list: Box::new(placeholder),
                                progress: None,
                            },
                            line: stage_line,
                        };
                        result = self.pipe_forward_apply(result, stage, stage_line)?;
                    // `pmap_reduce { MAP } { REDUCE }` — parallel map-reduce
                    } else if func_name == "pmap_reduce" {
                        let map_block = self.parse_block_or_bareword_block()?;
                        let reduce_block = if matches!(self.peek(), Token::LBrace) {
                            self.parse_block()?
                        } else {
                            self.expect(&Token::Comma)?;
                            self.parse_block_or_bareword_cmp_block()?
                        };
                        let placeholder = self.pipe_placeholder_list(stage_line);
                        let stage = Expr {
                            kind: ExprKind::PMapReduceExpr {
                                map_block,
                                reduce_block,
                                list: Box::new(placeholder),
                                progress: None,
                            },
                            line: stage_line,
                        };
                        result = self.pipe_forward_apply(result, stage, stage_line)?;
                    // Check if followed by a block (like `filter { }`, `sort { }`, `map { }`)
                    } else if matches!(self.peek(), Token::LBrace) {
                        // Parse as a block-taking builtin
                        self.pipe_rhs_depth = self.pipe_rhs_depth.saturating_add(1);
                        let stage = self.parse_thread_stage_with_block(&func_name, stage_line)?;
                        self.pipe_rhs_depth = self.pipe_rhs_depth.saturating_sub(1);
                        result = self.pipe_forward_apply(result, stage, stage_line)?;
                    } else if matches!(self.peek(), Token::LParen) {
                        // Special handling for join(sep) and split(pattern) in thread context.
                        // These take the threaded list/string as their data argument, not as $_.
                        if func_name == "join" {
                            self.advance(); // consume `(`
                            let separator = self.parse_assign_expr()?;
                            self.expect(&Token::RParen)?;
                            let placeholder = self.pipe_placeholder_list(stage_line);
                            let stage = Expr {
                                kind: ExprKind::JoinExpr {
                                    separator: Box::new(separator),
                                    list: Box::new(placeholder),
                                },
                                line: stage_line,
                            };
                            result = self.pipe_forward_apply(result, stage, stage_line)?;
                        } else if func_name == "split" {
                            self.advance(); // consume `(`
                            let pattern = self.parse_assign_expr()?;
                            let limit = if self.eat(&Token::Comma) {
                                Some(Box::new(self.parse_assign_expr()?))
                            } else {
                                None
                            };
                            self.expect(&Token::RParen)?;
                            let placeholder = Expr {
                                kind: ExprKind::ScalarVar("_".to_string()),
                                line: stage_line,
                            };
                            let stage = Expr {
                                kind: ExprKind::SplitExpr {
                                    pattern: Box::new(pattern),
                                    string: Box::new(placeholder),
                                    limit,
                                },
                                line: stage_line,
                            };
                            result = self.pipe_forward_apply(result, stage, stage_line)?;
                        } else {
                            // `name($_-bearing-args)` — parse explicit args, require at
                            // least one `$_` placeholder, then wrap as a `>{...}` block
                            // so the threaded value binds to `$_` at any position.
                            // Examples:
                            //   t 10 add2($_, 5) p      → add2(10, 5)
                            //   t 10 sub2(20, $_) p     → sub2(20, 10)
                            //   t 10 add3($_, 5, 10) p  → add3(10, 5, 10)
                            // To pass the threaded value as a sole arg, use bare form:
                            //   t 10 add2 p   (not `add2()`)
                            self.advance(); // consume `(`
                            let mut call_args = Vec::new();
                            while !matches!(self.peek(), Token::RParen | Token::Eof) {
                                call_args.push(self.parse_assign_expr()?);
                                if !self.eat(&Token::Comma) {
                                    break;
                                }
                            }
                            self.expect(&Token::RParen)?;
                            // If no `$_` placeholder, auto-inject threaded value as first arg.
                            // `t data to_file("/tmp/o.html")` → `to_file($_, "/tmp/o.html")`
                            if !call_args.iter().any(Self::expr_contains_topic_var) {
                                call_args.insert(
                                    0,
                                    Expr {
                                        kind: ExprKind::ScalarVar("_".to_string()),
                                        line: stage_line,
                                    },
                                );
                            }
                            let call_expr = Expr {
                                kind: ExprKind::FuncCall {
                                    name: func_name.clone(),
                                    args: call_args,
                                },
                                line: stage_line,
                            };
                            let code_ref = Expr {
                                kind: ExprKind::CodeRef {
                                    params: vec![],
                                    body: vec![Statement {
                                        label: None,
                                        kind: StmtKind::Expression(call_expr),
                                        line: stage_line,
                                    }],
                                },
                                line: stage_line,
                            };
                            result = self.pipe_forward_apply(result, code_ref, stage_line)?;
                        }
                    } else {
                        // Bare function name — handle unary builtins specially
                        result = self.thread_apply_bare_func(&func_name, result, stage_line)?;
                    }
                }
                // `/pattern/flags` — grep filter (desugar to `grep { /pattern/flags }`)
                Token::Regex(ref pattern, ref flags, delim) => {
                    let pattern = pattern.clone();
                    let flags = flags.clone();
                    self.advance();
                    result =
                        self.thread_regex_grep_stage(result, pattern, flags, delim, stage_line);
                }
                // Handle `/` that was lexed as Slash (division) because it followed a term.
                // In thread stage context, `/pattern/` should be a regex filter.
                Token::Slash => {
                    self.advance(); // consume opening /

                    // Special case: if next token is Ident("m") or similar followed by Regex,
                    // the lexer interpreted `/m/` as `/ m/pattern/` where `m/` started a new regex.
                    // We need to handle this: the pattern is just "m" (or whatever the ident is).
                    if let Token::Ident(ref ident_s) = self.peek().clone() {
                        if matches!(ident_s.as_str(), "m" | "s" | "tr" | "y" | "qr")
                            && matches!(self.peek_at(1), Token::Regex(..))
                        {
                            // The `m` (or s/tr/y/qr) is our pattern, the Regex token was misparsed
                            self.advance(); // consume the ident
                                            // The Token::Regex after it was a misparsed `m/...` - we need to
                                            // extract what would have been the closing `/` situation.
                                            // Actually, the lexer consumed everything. Let's just use the ident
                                            // as the pattern and expect a closing slash.
                            if let Token::Regex(ref misparsed_pattern, ref misparsed_flags, _) =
                                self.peek().clone()
                            {
                                // The misparsed regex ate our closing `/`.
                                // For `/m/`, lexer saw `m/` and parsed until next `/`, finding nothing or wrong content.
                                // Actually for `/m/ less`, after Slash, lexer sees `m`, then `/`,
                                // interprets as m// regex start, reads until next `/` (none) -> error.
                                // So we shouldn't reach here if there was an error.
                                // But if lexer succeeded parsing `m/ less/` as regex, we'd have wrong pattern.
                                // This is getting complicated. Let me try a different approach.
                                // Just consume the Regex token and issue a warning? No, let's reconstruct.
                                // Skip for now and fall through to manual parsing.
                                let _ = (misparsed_pattern, misparsed_flags);
                            }
                        }
                    }

                    // Manually parse the regex pattern from tokens until we hit another Slash
                    let mut pattern = String::new();
                    loop {
                        match self.peek().clone() {
                            Token::Slash => {
                                self.advance(); // consume closing /
                                break;
                            }
                            Token::Eof | Token::Semicolon | Token::Newline => {
                                return Err(self
                                    .syntax_err("Unterminated regex in thread stage", stage_line));
                            }
                            // Handle case where lexer misparsed m/pattern/ as Ident("m") + Regex
                            Token::Regex(ref inner_pattern, ref inner_flags, delim) => {
                                // This means `/m/` was lexed as Slash, then `m/` started a regex.
                                // The Regex token contains whatever was between the inner `m/` and closing `/`.
                                // For `/m/ less`, lexer would fail earlier. For `/m/i`, it might work weirdly.
                                // The safest: if we see a Regex token here and pattern is empty or just "m"/"s"/etc,
                                // treat the previous ident as the whole pattern and this Regex as misparsed.
                                // Actually, let's just prepend the ident we may have seen and use empty pattern.
                                // This is a lexer bug workaround.
                                if pattern.is_empty()
                                    || matches!(pattern.as_str(), "m" | "s" | "tr" | "y" | "qr")
                                {
                                    // The whole thing was probably `/X/` where X is m/s/tr/y/qr
                                    // and lexer misparsed. The Regex token is garbage.
                                    // Just use the ident as pattern and ignore this Regex.
                                    // But we already advanced past the ident...
                                    // This is messy. Let me try a cleaner approach.
                                    let _ = (inner_pattern, inner_flags, delim);
                                }
                                // For now, error out - this case is too complex
                                return Err(self.syntax_err(
                                    "Complex regex in thread stage - use m/pattern/ syntax instead",
                                    stage_line,
                                ));
                            }
                            Token::Ident(ref s) => {
                                pattern.push_str(s);
                                self.advance();
                            }
                            Token::Integer(n) => {
                                pattern.push_str(&n.to_string());
                                self.advance();
                            }
                            Token::ScalarVar(ref v) => {
                                pattern.push('$');
                                pattern.push_str(v);
                                self.advance();
                            }
                            Token::Dot => {
                                pattern.push('.');
                                self.advance();
                            }
                            Token::Star => {
                                pattern.push('*');
                                self.advance();
                            }
                            Token::Plus => {
                                pattern.push('+');
                                self.advance();
                            }
                            Token::Question => {
                                pattern.push('?');
                                self.advance();
                            }
                            Token::LParen => {
                                pattern.push('(');
                                self.advance();
                            }
                            Token::RParen => {
                                pattern.push(')');
                                self.advance();
                            }
                            Token::LBracket => {
                                pattern.push('[');
                                self.advance();
                            }
                            Token::RBracket => {
                                pattern.push(']');
                                self.advance();
                            }
                            Token::Backslash => {
                                pattern.push('\\');
                                self.advance();
                            }
                            Token::BitOr => {
                                pattern.push('|');
                                self.advance();
                            }
                            Token::Power => {
                                pattern.push_str("**");
                                self.advance();
                            }
                            Token::BitXor => {
                                pattern.push('^');
                                self.advance();
                            }
                            Token::Minus => {
                                pattern.push('-');
                                self.advance();
                            }
                            _ => {
                                return Err(self.syntax_err(
                                    format!("Unexpected token in regex pattern: {:?}", self.peek()),
                                    stage_line,
                                ));
                            }
                        }
                    }
                    // Parse optional flags (sequence of letters after closing /)
                    // Be careful: single letters like 'e' could be regex flags OR thread
                    // stages like `fore`/`e`. If followed by `{`, it's a stage, not a flag.
                    let mut flags = String::new();
                    if let Token::Ident(ref s) = self.peek().clone() {
                        let is_flag_only =
                            s.chars().all(|c| "gimsxecor".contains(c)) && s.len() <= 6;
                        let followed_by_brace = matches!(self.peek_at(1), Token::LBrace);
                        if is_flag_only && !followed_by_brace {
                            flags.push_str(s);
                            self.advance();
                        }
                    }
                    result = self.thread_regex_grep_stage(result, pattern, flags, '/', stage_line);
                }
                tok => {
                    return Err(self.syntax_err(
                        format!(
                            "thread: expected stage (ident, fn {{}}, s///, tr///, or /re/), got {:?}",
                            tok
                        ),
                        stage_line,
                    ));
                }
            };
            last_stage_end_line = self.prev_line();
        }

        if pipe_rhs_wrap {
            // Wrap as `fn { …stages threaded from $_[0]… }` so the outer
            // `pipe_forward_apply` can invoke it with `lhs` as the arg.
            let body_line = result.line;
            return Ok(Expr {
                kind: ExprKind::CodeRef {
                    params: vec![],
                    body: vec![Statement {
                        label: None,
                        kind: StmtKind::Expression(result),
                        line: body_line,
                    }],
                },
                line: _line,
            });
        }
        Ok(result)
    }

    /// Build a grep filter stage from a regex pattern for the thread macro.
    fn thread_regex_grep_stage(
        &self,
        list: Expr,
        pattern: String,
        flags: String,
        delim: char,
        line: usize,
    ) -> Expr {
        let topic = Expr {
            kind: ExprKind::ScalarVar("_".to_string()),
            line,
        };
        let match_expr = Expr {
            kind: ExprKind::Match {
                expr: Box::new(topic),
                pattern,
                flags,
                scalar_g: false,
                delim,
            },
            line,
        };
        let block = vec![Statement {
            label: None,
            kind: StmtKind::Expression(match_expr),
            line,
        }];
        Expr {
            kind: ExprKind::GrepExpr {
                block,
                list: Box::new(list),
                keyword: crate::ast::GrepBuiltinKeyword::Grep,
            },
            line,
        }
    }

    /// Check whether an expression contains a `$_` reference anywhere in its sub-tree.
    /// Used by the thread macro to validate `name(args)` call-stages: the threaded
    /// value is bound to `$_` via a wrapping CodeRef, so at least one `$_` placeholder
    /// must appear in the args, otherwise the threaded value is silently dropped.
    ///
    /// Implementation uses Rust's `Debug` to serialize the entire sub-tree once and
    /// scan for the canonical `ScalarVar("_")` representation. This avoids a
    /// per-variant walker that would need to be updated whenever new `ExprKind`
    /// variants are added (and would silently miss any it forgot to handle).
    /// Parse-time perf is non-critical and the AST is small at this scope.
    fn expr_contains_topic_var(e: &Expr) -> bool {
        format!("{:?}", e).contains("ScalarVar(\"_\")")
    }

    /// Apply a bare function name in thread context, handling unary builtins specially.
    fn thread_apply_bare_func(&self, name: &str, arg: Expr, line: usize) -> PerlResult<Expr> {
        let kind = match name {
            // String functions
            "uc" => ExprKind::Uc(Box::new(arg)),
            "lc" => ExprKind::Lc(Box::new(arg)),
            "ucfirst" | "ufc" => ExprKind::Ucfirst(Box::new(arg)),
            "lcfirst" | "lfc" => ExprKind::Lcfirst(Box::new(arg)),
            "fc" => ExprKind::Fc(Box::new(arg)),
            "chomp" => ExprKind::Chomp(Box::new(arg)),
            "chop" => ExprKind::Chop(Box::new(arg)),
            "length" => ExprKind::Length(Box::new(arg)),
            "len" | "cnt" => ExprKind::FuncCall {
                name: "count".to_string(),
                args: vec![arg],
            },
            "quotemeta" | "qm" => ExprKind::FuncCall {
                name: "quotemeta".to_string(),
                args: vec![arg],
            },
            // Numeric functions
            "abs" => ExprKind::Abs(Box::new(arg)),
            "int" => ExprKind::Int(Box::new(arg)),
            "sqrt" | "sq" => ExprKind::Sqrt(Box::new(arg)),
            "sin" => ExprKind::Sin(Box::new(arg)),
            "cos" => ExprKind::Cos(Box::new(arg)),
            "exp" => ExprKind::Exp(Box::new(arg)),
            "log" => ExprKind::Log(Box::new(arg)),
            "hex" => ExprKind::Hex(Box::new(arg)),
            "oct" => ExprKind::Oct(Box::new(arg)),
            "chr" => ExprKind::Chr(Box::new(arg)),
            "ord" => ExprKind::Ord(Box::new(arg)),
            // Type/ref functions
            "defined" | "def" => ExprKind::Defined(Box::new(arg)),
            "ref" => ExprKind::Ref(Box::new(arg)),
            "scalar" => ExprKind::ScalarContext(Box::new(arg)),
            // Array/hash functions
            "keys" => ExprKind::Keys(Box::new(arg)),
            "values" => ExprKind::Values(Box::new(arg)),
            "each" => ExprKind::Each(Box::new(arg)),
            "pop" => ExprKind::Pop(Box::new(arg)),
            "shift" => ExprKind::Shift(Box::new(arg)),
            "reverse" => {
                if !crate::compat_mode() {
                    return Err(
                        self.syntax_err("`reverse` is not valid stryke; use `rev` instead", line)
                    );
                }
                ExprKind::ReverseExpr(Box::new(arg))
            }
            "reversed" | "rv" | "rev" => ExprKind::Rev(Box::new(arg)),
            "sort" | "so" => ExprKind::SortExpr {
                cmp: None,
                list: Box::new(arg),
            },
            "uniq" | "distinct" | "uq" => ExprKind::FuncCall {
                name: "uniq".to_string(),
                args: vec![arg],
            },
            "trim" | "tm" => ExprKind::FuncCall {
                name: "trim".to_string(),
                args: vec![arg],
            },
            "flatten" | "fl" => ExprKind::FuncCall {
                name: "flatten".to_string(),
                args: vec![arg],
            },
            "compact" | "cpt" => ExprKind::FuncCall {
                name: "compact".to_string(),
                args: vec![arg],
            },
            "shuffle" | "shuf" => ExprKind::FuncCall {
                name: "shuffle".to_string(),
                args: vec![arg],
            },
            "frequencies" | "freq" | "frq" => ExprKind::FuncCall {
                name: "frequencies".to_string(),
                args: vec![arg],
            },
            "dedup" | "dup" => ExprKind::FuncCall {
                name: "dedup".to_string(),
                args: vec![arg],
            },
            "enumerate" | "en" => ExprKind::FuncCall {
                name: "enumerate".to_string(),
                args: vec![arg],
            },
            "lines" | "ln" => ExprKind::FuncCall {
                name: "lines".to_string(),
                args: vec![arg],
            },
            "words" | "wd" => ExprKind::FuncCall {
                name: "words".to_string(),
                args: vec![arg],
            },
            "chars" | "ch" => ExprKind::FuncCall {
                name: "chars".to_string(),
                args: vec![arg],
            },
            "digits" | "dg" => ExprKind::FuncCall {
                name: "digits".to_string(),
                args: vec![arg],
            },
            "letters" | "lts" => ExprKind::FuncCall {
                name: "letters".to_string(),
                args: vec![arg],
            },
            "letters_uc" => ExprKind::FuncCall {
                name: "letters_uc".to_string(),
                args: vec![arg],
            },
            "letters_lc" => ExprKind::FuncCall {
                name: "letters_lc".to_string(),
                args: vec![arg],
            },
            "punctuation" | "punct" => ExprKind::FuncCall {
                name: "punctuation".to_string(),
                args: vec![arg],
            },
            "sentences" | "sents" => ExprKind::FuncCall {
                name: "sentences".to_string(),
                args: vec![arg],
            },
            "paragraphs" | "paras" => ExprKind::FuncCall {
                name: "paragraphs".to_string(),
                args: vec![arg],
            },
            "sections" | "sects" => ExprKind::FuncCall {
                name: "sections".to_string(),
                args: vec![arg],
            },
            "numbers" | "nums" => ExprKind::FuncCall {
                name: "numbers".to_string(),
                args: vec![arg],
            },
            "graphemes" | "grs" => ExprKind::FuncCall {
                name: "graphemes".to_string(),
                args: vec![arg],
            },
            "columns" | "cols" => ExprKind::FuncCall {
                name: "columns".to_string(),
                args: vec![arg],
            },
            // File functions
            "slurp" | "sl" => ExprKind::Slurp(Box::new(arg)),
            "chdir" => ExprKind::Chdir(Box::new(arg)),
            "stat" => ExprKind::Stat(Box::new(arg)),
            "lstat" => ExprKind::Lstat(Box::new(arg)),
            "readlink" => ExprKind::Readlink(Box::new(arg)),
            "readdir" => ExprKind::Readdir(Box::new(arg)),
            "close" => ExprKind::Close(Box::new(arg)),
            "basename" | "bn" => ExprKind::FuncCall {
                name: "basename".to_string(),
                args: vec![arg],
            },
            "dirname" | "dn" => ExprKind::FuncCall {
                name: "dirname".to_string(),
                args: vec![arg],
            },
            "realpath" | "rp" => ExprKind::FuncCall {
                name: "realpath".to_string(),
                args: vec![arg],
            },
            "which" | "wh" => ExprKind::FuncCall {
                name: "which".to_string(),
                args: vec![arg],
            },
            // Other
            "eval" => ExprKind::Eval(Box::new(arg)),
            "require" => ExprKind::Require(Box::new(arg)),
            "study" => ExprKind::Study(Box::new(arg)),
            // Case conversion
            "snake_case" | "sc" => ExprKind::FuncCall {
                name: "snake_case".to_string(),
                args: vec![arg],
            },
            "camel_case" | "cc" => ExprKind::FuncCall {
                name: "camel_case".to_string(),
                args: vec![arg],
            },
            "kebab_case" | "kc" => ExprKind::FuncCall {
                name: "kebab_case".to_string(),
                args: vec![arg],
            },
            // Serialization
            "to_json" | "tj" => ExprKind::FuncCall {
                name: "to_json".to_string(),
                args: vec![arg],
            },
            "to_yaml" | "ty" => ExprKind::FuncCall {
                name: "to_yaml".to_string(),
                args: vec![arg],
            },
            "to_toml" | "tt" => ExprKind::FuncCall {
                name: "to_toml".to_string(),
                args: vec![arg],
            },
            "to_csv" | "tc" => ExprKind::FuncCall {
                name: "to_csv".to_string(),
                args: vec![arg],
            },
            "to_xml" | "tx" => ExprKind::FuncCall {
                name: "to_xml".to_string(),
                args: vec![arg],
            },
            "to_html" | "th" => ExprKind::FuncCall {
                name: "to_html".to_string(),
                args: vec![arg],
            },
            "to_markdown" | "to_md" | "tmd" => ExprKind::FuncCall {
                name: "to_markdown".to_string(),
                args: vec![arg],
            },
            "xopen" | "xo" => ExprKind::FuncCall {
                name: "xopen".to_string(),
                args: vec![arg],
            },
            "clip" | "clipboard" | "pbcopy" => ExprKind::FuncCall {
                name: "clip".to_string(),
                args: vec![arg],
            },
            "to_table" | "table" | "tbl" => ExprKind::FuncCall {
                name: "to_table".to_string(),
                args: vec![arg],
            },
            "sparkline" | "spark" => ExprKind::FuncCall {
                name: "sparkline".to_string(),
                args: vec![arg],
            },
            "bar_chart" | "bars" => ExprKind::FuncCall {
                name: "bar_chart".to_string(),
                args: vec![arg],
            },
            "flame" | "flamechart" => ExprKind::FuncCall {
                name: "flame".to_string(),
                args: vec![arg],
            },
            "ddump" | "dd" => ExprKind::FuncCall {
                name: "ddump".to_string(),
                args: vec![arg],
            },
            "say" => {
                if !crate::compat_mode() {
                    return Err(self.syntax_err("`say` is not valid stryke; use `p` instead", line));
                }
                ExprKind::Say {
                    handle: None,
                    args: vec![arg],
                }
            }
            "p" => ExprKind::Say {
                handle: None,
                args: vec![arg],
            },
            "print" => ExprKind::Print {
                handle: None,
                args: vec![arg],
            },
            "warn" => ExprKind::Warn(vec![arg]),
            "die" => ExprKind::Die(vec![arg]),
            "stringify" | "str" => ExprKind::FuncCall {
                name: "stringify".to_string(),
                args: vec![arg],
            },
            "json_decode" | "jd" => ExprKind::FuncCall {
                name: "json_decode".to_string(),
                args: vec![arg],
            },
            "yaml_decode" | "yd" => ExprKind::FuncCall {
                name: "yaml_decode".to_string(),
                args: vec![arg],
            },
            "toml_decode" | "td" => ExprKind::FuncCall {
                name: "toml_decode".to_string(),
                args: vec![arg],
            },
            "xml_decode" | "xd" => ExprKind::FuncCall {
                name: "xml_decode".to_string(),
                args: vec![arg],
            },
            "json_encode" | "je" => ExprKind::FuncCall {
                name: "json_encode".to_string(),
                args: vec![arg],
            },
            "yaml_encode" | "ye" => ExprKind::FuncCall {
                name: "yaml_encode".to_string(),
                args: vec![arg],
            },
            "toml_encode" | "te" => ExprKind::FuncCall {
                name: "toml_encode".to_string(),
                args: vec![arg],
            },
            "xml_encode" | "xe" => ExprKind::FuncCall {
                name: "xml_encode".to_string(),
                args: vec![arg],
            },
            // Encoding
            "base64_encode" | "b64e" => ExprKind::FuncCall {
                name: "base64_encode".to_string(),
                args: vec![arg],
            },
            "base64_decode" | "b64d" => ExprKind::FuncCall {
                name: "base64_decode".to_string(),
                args: vec![arg],
            },
            "hex_encode" | "hxe" => ExprKind::FuncCall {
                name: "hex_encode".to_string(),
                args: vec![arg],
            },
            "hex_decode" | "hxd" => ExprKind::FuncCall {
                name: "hex_decode".to_string(),
                args: vec![arg],
            },
            "url_encode" | "uri_escape" | "ue" => ExprKind::FuncCall {
                name: "url_encode".to_string(),
                args: vec![arg],
            },
            "url_decode" | "uri_unescape" | "ud" => ExprKind::FuncCall {
                name: "url_decode".to_string(),
                args: vec![arg],
            },
            "gzip" | "gz" => ExprKind::FuncCall {
                name: "gzip".to_string(),
                args: vec![arg],
            },
            "gunzip" | "ugz" => ExprKind::FuncCall {
                name: "gunzip".to_string(),
                args: vec![arg],
            },
            "zstd" | "zst" => ExprKind::FuncCall {
                name: "zstd".to_string(),
                args: vec![arg],
            },
            "zstd_decode" | "uzst" => ExprKind::FuncCall {
                name: "zstd_decode".to_string(),
                args: vec![arg],
            },
            // Crypto
            "sha256" | "s256" => ExprKind::FuncCall {
                name: "sha256".to_string(),
                args: vec![arg],
            },
            "sha1" | "s1" => ExprKind::FuncCall {
                name: "sha1".to_string(),
                args: vec![arg],
            },
            "md5" | "m5" => ExprKind::FuncCall {
                name: "md5".to_string(),
                args: vec![arg],
            },
            "uuid" | "uid" => ExprKind::FuncCall {
                name: "uuid".to_string(),
                args: vec![arg],
            },
            // Datetime
            "datetime_utc" | "utc" => ExprKind::FuncCall {
                name: "datetime_utc".to_string(),
                args: vec![arg],
            },
            // Bare `e` / `fore` / `ep` in thread context: foreach element, say it.
            // `t @list e` == `@list |> e p` == `@list |> ep` == foreach (@list) { say }
            "e" | "fore" | "ep" => ExprKind::ForEachExpr {
                block: vec![Statement {
                    label: None,
                    kind: StmtKind::Expression(Expr {
                        kind: ExprKind::Say {
                            handle: None,
                            args: vec![Expr {
                                kind: ExprKind::ScalarVar("_".into()),
                                line,
                            }],
                        },
                        line,
                    }),
                    line,
                }],
                list: Box::new(arg),
            },
            // Default: generic function call
            _ => ExprKind::FuncCall {
                name: name.to_string(),
                args: vec![arg],
            },
        };
        Ok(Expr { kind, line })
    }

    /// Parse a thread stage that has a block: `map { }`, `filter { }`, `sort { }`, etc.
    /// In thread context, we only parse the block - the list comes from the piped result.
    fn parse_thread_stage_with_block(&mut self, name: &str, line: usize) -> PerlResult<Expr> {
        let block = self.parse_block()?;
        // Use a placeholder for the list - pipe_forward_apply will replace it
        let placeholder = self.pipe_placeholder_list(line);

        match name {
            "map" | "flat_map" | "maps" | "flat_maps" => {
                let flatten_array_refs = matches!(name, "flat_map" | "flat_maps");
                let stream = matches!(name, "maps" | "flat_maps");
                Ok(Expr {
                    kind: ExprKind::MapExpr {
                        block,
                        list: Box::new(placeholder),
                        flatten_array_refs,
                        stream,
                    },
                    line,
                })
            }
            "grep" | "greps" | "filter" | "fi" | "find_all" | "gr" => {
                let keyword = match name {
                    "grep" | "gr" => crate::ast::GrepBuiltinKeyword::Grep,
                    "greps" => crate::ast::GrepBuiltinKeyword::Greps,
                    "filter" | "fi" => crate::ast::GrepBuiltinKeyword::Filter,
                    "find_all" => crate::ast::GrepBuiltinKeyword::FindAll,
                    _ => unreachable!(),
                };
                Ok(Expr {
                    kind: ExprKind::GrepExpr {
                        block,
                        list: Box::new(placeholder),
                        keyword,
                    },
                    line,
                })
            }
            "sort" | "so" => Ok(Expr {
                kind: ExprKind::SortExpr {
                    cmp: Some(SortComparator::Block(block)),
                    list: Box::new(placeholder),
                },
                line,
            }),
            "reduce" | "rd" => Ok(Expr {
                kind: ExprKind::ReduceExpr {
                    block,
                    list: Box::new(placeholder),
                },
                line,
            }),
            "fore" | "e" | "ep" => Ok(Expr {
                kind: ExprKind::ForEachExpr {
                    block,
                    list: Box::new(placeholder),
                },
                line,
            }),
            "pmap" | "pflat_map" | "pmaps" | "pflat_maps" => Ok(Expr {
                kind: ExprKind::PMapExpr {
                    block,
                    list: Box::new(placeholder),
                    progress: None,
                    flat_outputs: name == "pflat_map" || name == "pflat_maps",
                    on_cluster: None,
                    stream: name == "pmaps" || name == "pflat_maps",
                },
                line,
            }),
            "pgrep" | "pgreps" => Ok(Expr {
                kind: ExprKind::PGrepExpr {
                    block,
                    list: Box::new(placeholder),
                    progress: None,
                    stream: name == "pgreps",
                },
                line,
            }),
            "pfor" => Ok(Expr {
                kind: ExprKind::PForExpr {
                    block,
                    list: Box::new(placeholder),
                    progress: None,
                },
                line,
            }),
            "preduce" => Ok(Expr {
                kind: ExprKind::PReduceExpr {
                    block,
                    list: Box::new(placeholder),
                    progress: None,
                },
                line,
            }),
            "pcache" => Ok(Expr {
                kind: ExprKind::PcacheExpr {
                    block,
                    list: Box::new(placeholder),
                    progress: None,
                },
                line,
            }),
            "psort" => Ok(Expr {
                kind: ExprKind::PSortExpr {
                    cmp: Some(block),
                    list: Box::new(placeholder),
                    progress: None,
                },
                line,
            }),
            _ => {
                // Generic: parse block and treat as FuncCall with code ref arg
                let code_ref = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.to_string(),
                        args: vec![code_ref],
                    },
                    line,
                })
            }
        }
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
            Token::Regex(pattern, flags, _delim) => {
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
            } if flags.contains('g') => {
                *scalar_g = true;
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
            if crate::compat_mode() {
                return Err(self.syntax_err(
                    "`if let` is a stryke extension (disabled by --compat)",
                    line,
                ));
            }
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
            if crate::compat_mode() {
                return Err(self.syntax_err(
                    "`while let` is a stryke extension (disabled by --compat)",
                    line,
                ));
            }
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
            Token::ArrayVar(_) | Token::HashVar(_) => true,
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
                                "`{name}` cannot start a stryke sub signature (use legacy prototype `($$)` etc.)"
                            ),
                            self.peek_line(),
                        ));
                    }
                    self.advance();
                    let ty = if self.eat(&Token::Colon) {
                        match self.peek() {
                            Token::Ident(ref tname) => {
                                let tname = tname.clone();
                                self.advance();
                                Some(match tname.as_str() {
                                    "Int" => PerlTypeName::Int,
                                    "Str" => PerlTypeName::Str,
                                    "Float" => PerlTypeName::Float,
                                    "Bool" => PerlTypeName::Bool,
                                    "Array" => PerlTypeName::Array,
                                    "Hash" => PerlTypeName::Hash,
                                    "Ref" => PerlTypeName::Ref,
                                    "Any" => PerlTypeName::Any,
                                    _ => PerlTypeName::Struct(tname),
                                })
                            }
                            _ => {
                                return Err(self.syntax_err(
                                    "expected type name after `:` in sub signature",
                                    self.peek_line(),
                                ));
                            }
                        }
                    } else {
                        None
                    };
                    // Check for default value: `$x = expr`
                    let default = if self.eat(&Token::Assign) {
                        Some(Box::new(self.parse_ternary()?))
                    } else {
                        None
                    };
                    params.push(SubSigParam::Scalar(name, ty, default));
                }
                Token::ArrayVar(name) => {
                    self.advance();
                    let default = if self.eat(&Token::Assign) {
                        Some(Box::new(self.parse_ternary()?))
                    } else {
                        None
                    };
                    params.push(SubSigParam::Array(name, default));
                }
                Token::HashVar(name) => {
                    self.advance();
                    let default = if self.eat(&Token::Assign) {
                        Some(Box::new(self.parse_ternary()?))
                    } else {
                        None
                    };
                    params.push(SubSigParam::Hash(name, default));
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

    /// Optional `sub` parens: either a Perl 5 prototype string or a stryke **`$name` / `{ k => $v }`** signature.
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

    fn parse_sub_decl(&mut self, is_sub_keyword: bool) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'sub' or 'fn'
        match self.peek().clone() {
            Token::Ident(_) => {
                let name = self.parse_package_qualified_identifier()?;
                if !crate::compat_mode() {
                    self.check_udf_shadows_builtin(&name, line)?;
                }
                self.declared_subs.insert(name.clone());
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
                // In non-compat mode, `fn {}` anonymous is not allowed — must use `fn {}`
                if is_sub_keyword && !crate::compat_mode() {
                    return Err(self.syntax_err(
                        "`fn {}` anonymous subroutine is not valid stryke; use `fn {}` instead",
                        line,
                    ));
                }
                // Statement-level anonymous sub: `fn { }`, `sub () { }`, `sub :lvalue { }`
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

    /// `struct Name { field => Type, ... ; fn method { } }`
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
        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Check for method definition: `fn name { }` or `sub name { }`
            let is_method = match self.peek() {
                Token::Ident(s) => s == "fn" || s == "sub",
                _ => false,
            };
            if is_method {
                self.advance(); // fn/sub
                let method_name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (tok, err_line) => {
                        return Err(self
                            .syntax_err(format!("Expected method name, got {:?}", tok), err_line))
                    }
                };
                // Parse optional signature: `($self, $arg: Type, ...)`
                let params = if self.eat(&Token::LParen) {
                    let p = self.parse_sub_signature_param_list()?;
                    self.expect(&Token::RParen)?;
                    p
                } else {
                    Vec::new()
                };
                // parse_block handles its own { } delimiters
                let body = self.parse_block()?;
                methods.push(crate::ast::StructMethod {
                    name: method_name,
                    params,
                    body,
                });
                // Optional trailing comma/semicolon after method
                self.eat(&Token::Comma);
                self.eat(&Token::Semicolon);
                continue;
            }

            let field_name = match self.advance() {
                (Token::Ident(n), _) => n,
                (tok, err_line) => {
                    return Err(
                        self.syntax_err(format!("Expected field name, got {:?}", tok), err_line)
                    )
                }
            };
            // Support both `field => Type` and bare `field` (implies Any type)
            let ty = if self.eat(&Token::FatArrow) {
                self.parse_type_name()?
            } else {
                crate::ast::PerlTypeName::Any
            };
            let default = if self.eat(&Token::Assign) {
                // Use parse_ternary to avoid consuming commas (next field separator)
                Some(self.parse_ternary()?)
            } else {
                None
            };
            fields.push(StructField {
                name: field_name,
                ty,
                default,
            });
            if !self.eat(&Token::Comma) {
                // Also allow semicolons as field separators
                self.eat(&Token::Semicolon);
            }
        }
        self.expect(&Token::RBrace)?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::StructDecl {
                def: StructDef {
                    name,
                    fields,
                    methods,
                },
            },
            line,
        })
    }

    /// `enum Name { Variant1, Variant2 => Type, ... }`
    fn parse_enum_decl(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // enum
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, err_line) => {
                return Err(self.syntax_err(format!("Expected enum name, got {:?}", tok), err_line))
            }
        };
        self.expect(&Token::LBrace)?;
        let mut variants = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let variant_name = match self.advance() {
                (Token::Ident(n), _) => n,
                (tok, err_line) => {
                    return Err(
                        self.syntax_err(format!("Expected variant name, got {:?}", tok), err_line)
                    )
                }
            };
            let ty = if self.eat(&Token::FatArrow) {
                Some(self.parse_type_name()?)
            } else {
                None
            };
            variants.push(EnumVariant {
                name: variant_name,
                ty,
            });
            if !self.eat(&Token::Comma) {
                self.eat(&Token::Semicolon);
            }
        }
        self.expect(&Token::RBrace)?;
        self.eat(&Token::Semicolon);
        Ok(Statement {
            label: None,
            kind: StmtKind::EnumDecl {
                def: EnumDef { name, variants },
            },
            line,
        })
    }

    /// `[abstract|final] class Name extends Parent impl Trait { fields; methods }`
    fn parse_class_decl(&mut self, is_abstract: bool, is_final: bool) -> PerlResult<Statement> {
        use crate::ast::{ClassDef, ClassField, ClassMethod, ClassStaticField, Visibility};
        let line = self.peek_line();
        self.advance(); // class
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, err_line) => {
                return Err(self.syntax_err(format!("Expected class name, got {:?}", tok), err_line))
            }
        };

        // Parse `extends Parent1, Parent2`
        let mut extends = Vec::new();
        if matches!(self.peek(), Token::Ident(ref s) if s == "extends") {
            self.advance(); // extends
            loop {
                match self.advance() {
                    (Token::Ident(parent), _) => extends.push(parent),
                    (tok, err_line) => {
                        return Err(self.syntax_err(
                            format!("Expected parent class name after `extends`, got {:?}", tok),
                            err_line,
                        ))
                    }
                }
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }

        // Parse `impl Trait1, Trait2`
        let mut implements = Vec::new();
        if matches!(self.peek(), Token::Ident(ref s) if s == "impl") {
            self.advance(); // impl
            loop {
                match self.advance() {
                    (Token::Ident(trait_name), _) => implements.push(trait_name),
                    (tok, err_line) => {
                        return Err(self.syntax_err(
                            format!("Expected trait name after `impl`, got {:?}", tok),
                            err_line,
                        ))
                    }
                }
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }

        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut static_fields = Vec::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Check for visibility modifier
            let visibility = match self.peek() {
                Token::Ident(ref s) if s == "pub" => {
                    self.advance();
                    Visibility::Public
                }
                Token::Ident(ref s) if s == "priv" => {
                    self.advance();
                    Visibility::Private
                }
                Token::Ident(ref s) if s == "prot" => {
                    self.advance();
                    Visibility::Protected
                }
                _ => Visibility::Public, // default public
            };

            // Check for static field: `static name: Type = default`
            if matches!(self.peek(), Token::Ident(ref s) if s == "static") {
                self.advance(); // static

                // Could be a static method (`static fn`) or static field
                if matches!(self.peek(), Token::Ident(ref s) if s == "fn" || s == "sub") {
                    // static fn is same as fn Self.name — handled below but not here
                    return Err(self.syntax_err(
                        "use `fn Self.name` for static methods, not `static fn`",
                        self.peek_line(),
                    ));
                }

                let field_name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (tok, err_line) => {
                        return Err(self.syntax_err(
                            format!("Expected static field name, got {:?}", tok),
                            err_line,
                        ))
                    }
                };

                let ty = if self.eat(&Token::Colon) {
                    self.parse_type_name()?
                } else {
                    crate::ast::PerlTypeName::Any
                };

                let default = if self.eat(&Token::Assign) {
                    Some(self.parse_ternary()?)
                } else {
                    None
                };

                static_fields.push(ClassStaticField {
                    name: field_name,
                    ty,
                    visibility,
                    default,
                });

                if !self.eat(&Token::Comma) {
                    self.eat(&Token::Semicolon);
                }
                continue;
            }

            // Check for `final` modifier before fn
            let method_is_final = matches!(self.peek(), Token::Ident(ref s) if s == "final");
            if method_is_final {
                self.advance(); // final
            }

            // Check for method: `fn name` or `fn Self.name` (static)
            let is_method = matches!(self.peek(), Token::Ident(ref s) if s == "fn" || s == "sub");
            if is_method {
                self.advance(); // fn/sub

                // Check for static method: `fn Self.name`
                let is_static = matches!(self.peek(), Token::Ident(ref s) if s == "Self");
                if is_static {
                    self.advance(); // Self
                    self.expect(&Token::Dot)?;
                }

                let method_name = match self.advance() {
                    (Token::Ident(n), _) => n,
                    (tok, err_line) => {
                        return Err(self
                            .syntax_err(format!("Expected method name, got {:?}", tok), err_line))
                    }
                };

                // Parse optional signature
                let params = if self.eat(&Token::LParen) {
                    let p = self.parse_sub_signature_param_list()?;
                    self.expect(&Token::RParen)?;
                    p
                } else {
                    Vec::new()
                };

                // Body is optional (abstract method in trait has no body)
                let body = if matches!(self.peek(), Token::LBrace) {
                    Some(self.parse_block()?)
                } else {
                    None
                };

                methods.push(ClassMethod {
                    name: method_name,
                    params,
                    body,
                    visibility,
                    is_static,
                    is_final: method_is_final,
                });
                self.eat(&Token::Comma);
                self.eat(&Token::Semicolon);
                continue;
            } else if method_is_final {
                return Err(self.syntax_err("`final` must be followed by `fn`", self.peek_line()));
            }

            // Parse field: `name: Type = default`
            let field_name = match self.advance() {
                (Token::Ident(n), _) => n,
                (tok, err_line) => {
                    return Err(
                        self.syntax_err(format!("Expected field name, got {:?}", tok), err_line)
                    )
                }
            };

            // Type after colon: `name: Type`
            let ty = if self.eat(&Token::Colon) {
                self.parse_type_name()?
            } else {
                crate::ast::PerlTypeName::Any
            };

            // Default value after `=`
            let default = if self.eat(&Token::Assign) {
                Some(self.parse_ternary()?)
            } else {
                None
            };

            fields.push(ClassField {
                name: field_name,
                ty,
                visibility,
                default,
            });

            if !self.eat(&Token::Comma) {
                self.eat(&Token::Semicolon);
            }
        }

        self.expect(&Token::RBrace)?;
        self.eat(&Token::Semicolon);

        Ok(Statement {
            label: None,
            kind: StmtKind::ClassDecl {
                def: ClassDef {
                    name,
                    is_abstract,
                    is_final,
                    extends,
                    implements,
                    fields,
                    methods,
                    static_fields,
                },
            },
            line,
        })
    }

    /// `trait Name { fn required; fn with_default { } }`
    fn parse_trait_decl(&mut self) -> PerlResult<Statement> {
        use crate::ast::{ClassMethod, TraitDef, Visibility};
        let line = self.peek_line();
        self.advance(); // trait
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, err_line) => {
                return Err(self.syntax_err(format!("Expected trait name, got {:?}", tok), err_line))
            }
        };

        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Optional visibility
            let visibility = match self.peek() {
                Token::Ident(ref s) if s == "pub" => {
                    self.advance();
                    Visibility::Public
                }
                Token::Ident(ref s) if s == "priv" => {
                    self.advance();
                    Visibility::Private
                }
                Token::Ident(ref s) if s == "prot" => {
                    self.advance();
                    Visibility::Protected
                }
                _ => Visibility::Public,
            };

            // Expect `fn` or `sub`
            if !matches!(self.peek(), Token::Ident(ref s) if s == "fn" || s == "sub") {
                return Err(self.syntax_err("Expected `fn` in trait definition", self.peek_line()));
            }
            self.advance(); // fn/sub

            let method_name = match self.advance() {
                (Token::Ident(n), _) => n,
                (tok, err_line) => {
                    return Err(
                        self.syntax_err(format!("Expected method name, got {:?}", tok), err_line)
                    )
                }
            };

            // Optional signature
            let params = if self.eat(&Token::LParen) {
                let p = self.parse_sub_signature_param_list()?;
                self.expect(&Token::RParen)?;
                p
            } else {
                Vec::new()
            };

            // Body is optional (no body = abstract/required method)
            let body = if matches!(self.peek(), Token::LBrace) {
                Some(self.parse_block()?)
            } else {
                None
            };

            methods.push(ClassMethod {
                name: method_name,
                params,
                body,
                visibility,
                is_static: false,
                is_final: false,
            });

            self.eat(&Token::Comma);
            self.eat(&Token::Semicolon);
        }

        self.expect(&Token::RBrace)?;
        self.eat(&Token::Semicolon);

        Ok(Statement {
            label: None,
            kind: StmtKind::TraitDecl {
                def: TraitDef { name, methods },
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
        let tmp = format!("__stryke_ds_{}", self.alloc_desugar_tmp());
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
        let tmp = format!("__stryke_ds_{}", self.alloc_desugar_tmp());
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
            // Validate assignment for single variable declarations (not destructuring)
            // `my ($a, $b) = (1, 2)` is destructuring, not scalar-from-list
            if !crate::compat_mode() && decls.len() == 1 {
                let decl = &decls[0];
                let target_kind = match decl.sigil {
                    Sigil::Scalar => ExprKind::ScalarVar(decl.name.clone()),
                    Sigil::Array => ExprKind::ArrayVar(decl.name.clone()),
                    Sigil::Hash => ExprKind::HashVar(decl.name.clone()),
                    Sigil::Typeglob => {
                        // Skip validation for typeglob
                        if decls.len() == 1 {
                            decls[0].initializer = Some(val);
                        } else {
                            for d in &mut decls {
                                d.initializer = Some(val.clone());
                            }
                        }
                        return Ok(Statement {
                            label: None,
                            kind: match keyword {
                                "my" => StmtKind::My(decls),
                                "mysync" => StmtKind::MySync(decls),
                                "our" => StmtKind::Our(decls),
                                "local" => StmtKind::Local(decls),
                                "state" => StmtKind::State(decls),
                                _ => unreachable!(),
                            },
                            line,
                        });
                    }
                };
                let target = Expr {
                    kind: target_kind,
                    line,
                };
                self.validate_assignment(&target, &val, line)?;
            }
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
            (Token::HashVar(name), line) => {
                if !crate::compat_mode() {
                    self.check_hash_shadows_reserved(&name, line)?;
                }
                VarDecl {
                    sigil: Sigil::Hash,
                    name,
                    initializer: None,
                    frozen: false,
                    type_annotation: None,
                }
            }
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
            // `my ($a, undef, $c) = (1, 2, 3)` — Perl idiom for discarding a
            // slot in a list assignment. The interpreter treats `undef`-named
            // scalar decls as throwaway: declared into a unique sink so the
            // distribute-to-decls loop advances past the slot.
            (Token::Ident(ref kw), _) if kw == "undef" => VarDecl {
                sigil: Sigil::Scalar,
                // Synthesize a name that user code cannot reference. Each
                // sink slot in a list-assign gets its own unique name so the
                // declarations don't collide.
                name: format!("__undef_sink_{}", self.pos),
                initializer: None,
                frozen: false,
                type_annotation: None,
            },
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
        match self.advance() {
            (Token::Ident(name), _) => match name.as_str() {
                "Int" => Ok(PerlTypeName::Int),
                "Str" => Ok(PerlTypeName::Str),
                "Float" => Ok(PerlTypeName::Float),
                "Bool" => Ok(PerlTypeName::Bool),
                "Array" => Ok(PerlTypeName::Array),
                "Hash" => Ok(PerlTypeName::Hash),
                "Ref" => Ok(PerlTypeName::Ref),
                "Any" => Ok(PerlTypeName::Any),
                _ => Ok(PerlTypeName::Struct(name)),
            },
            (tok, err_line) => Err(self.syntax_err(
                format!("Expected type name after `:`, got {:?}", tok),
                err_line,
            )),
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
                if !matches!(self.peek(), Token::Semicolon | Token::Eof)
                    && !self.next_is_new_statement_start(tok_line)
                {
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
            (Token::Ident(n), tok_line) => (n, tok_line),
            (tok, line) => {
                return Err(self.syntax_err(
                    format!("Expected module name after no, got {:?}", tok),
                    line,
                ))
            }
        };
        let (module_name, tok_line) = module;
        let mut full_name = module_name;
        while self.eat(&Token::PackageSep) {
            if let (Token::Ident(part), _) = self.advance() {
                full_name = format!("{}::{}", full_name, part);
            }
        }
        let mut imports = Vec::new();
        if !matches!(self.peek(), Token::Semicolon | Token::Eof)
            && !self.next_is_new_statement_start(tok_line)
        {
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
                // Desugar `$obj->field = value` into `$obj->field(value)` (setter call)
                if let ExprKind::MethodCall { ref args, .. } = expr.kind {
                    if args.is_empty() {
                        // Destructure again to take ownership
                        let ExprKind::MethodCall {
                            object,
                            method,
                            super_call,
                            ..
                        } = expr.kind
                        else {
                            unreachable!()
                        };
                        return Ok(Expr {
                            kind: ExprKind::MethodCall {
                                object,
                                method,
                                args: vec![right],
                                super_call,
                            },
                            line,
                        });
                    }
                }
                self.validate_assignment(&expr, &right, line)?;
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
            if crate::compat_mode() {
                return Err(self.syntax_err(
                    "pipe-forward operator `|>` is a stryke extension (disabled by --compat)",
                    left.line,
                ));
            }
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
                    | "list_size" | "count" | "size" | "cnt" | "len" | "with_index" | "shuffle"
                    | "shuffled" | "frequencies" | "freq" | "interleave" | "ddump"
                    | "stringify" | "str" | "lines" | "words" | "chars" | "digits" | "letters"
                    | "letters_uc" | "letters_lc" | "punctuation" | "numbers" | "graphemes"
                    | "columns" | "sentences" | "paragraphs" | "sections" | "trim" | "avg"
                    | "to_json" | "to_csv" | "to_toml" | "to_yaml" | "to_xml" | "to_html"
                    | "from_json" | "from_csv" | "from_toml" | "from_yaml" | "from_xml"
                    | "to_markdown" | "to_table" | "xopen" | "clip" | "sparkline" | "bar_chart"
                    | "flame" | "stddev" | "squared" | "sq" | "square" | "cubed" | "cb"
                    | "cube" | "normalize" | "snake_case" | "camel_case" | "kebab_case" => {
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
                    "grep_v" | "pluck" | "tee" | "nth" | "chunk" => {
                        // data |> grep_v "pattern" → grep_v("pattern", data...)
                        // data |> pluck "key" → pluck("key", data...)
                        // data |> tee "file" → tee("file", data...)
                        // data |> nth N → nth(N, data...)
                        // data |> chunk N → chunk(N, data...)
                        args.push(lhs);
                    }
                    "enumerate" | "dedup" => {
                        // data |> enumerate → enumerate(data)
                        // data |> dedup → dedup(data)
                        args.insert(0, lhs);
                    }
                    "clamp" => {
                        // data |> clamp MIN, MAX → clamp(MIN, MAX, data...)
                        args.push(lhs);
                    }
                    "pfirst" | "pany" | "any" | "all" | "none" | "first" | "take_while"
                    | "drop_while" | "skip_while" | "reject" | "tap" | "peek" | "group_by"
                    | "chunk_by" | "partition" | "min_by" | "max_by" | "zip_with" | "count_by" => {
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
            ExprKind::FilesfRecursive(mut args) => {
                args.insert(0, lhs);
                ExprKind::FilesfRecursive(args)
            }
            ExprKind::Dirs(mut args) => {
                args.insert(0, lhs);
                ExprKind::Dirs(args)
            }
            ExprKind::DirsRecursive(mut args) => {
                args.insert(0, lhs);
                ExprKind::DirsRecursive(args)
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
            ExprKind::Rev(_) => ExprKind::Rev(Box::new(lhs)),
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
            ExprKind::Rand(_) => ExprKind::Rand(Some(Box::new(lhs))),
            ExprKind::Srand(_) => ExprKind::Srand(Some(Box::new(lhs))),
            ExprKind::Pos(_) => ExprKind::Pos(Some(Box::new(lhs))),
            ExprKind::Exit(_) => ExprKind::Exit(Some(Box::new(lhs))),

            // ── Higher-order / list-taking forms: replace the `list` slot ──────
            ExprKind::MapExpr {
                block,
                list: _,
                flatten_array_refs,
                stream,
            } => ExprKind::MapExpr {
                block,
                list: Box::new(lhs),
                flatten_array_refs,
                stream,
            },
            ExprKind::MapExprComma {
                expr,
                list: _,
                flatten_array_refs,
                stream,
            } => ExprKind::MapExprComma {
                expr,
                list: Box::new(lhs),
                flatten_array_refs,
                stream,
            },
            ExprKind::GrepExpr {
                block,
                list: _,
                keyword,
            } => ExprKind::GrepExpr {
                block,
                list: Box::new(lhs),
                keyword,
            },
            ExprKind::GrepExprComma {
                expr,
                list: _,
                keyword,
            } => ExprKind::GrepExprComma {
                expr,
                list: Box::new(lhs),
                keyword,
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
                stream,
            } => ExprKind::PMapExpr {
                block,
                list: Box::new(lhs),
                progress,
                flat_outputs,
                on_cluster,
                stream,
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
                stream,
            } => ExprKind::PGrepExpr {
                block,
                list: Box::new(lhs),
                progress,
                stream,
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

            // ── Regex ops: pipe the subject — `$str |> s/\n//g` ────────────────
            //    Auto-inject `r` flag so the substitution returns the modified
            //    string instead of the match count (non-destructive / Perl /r).
            ExprKind::Substitution {
                pattern,
                replacement,
                mut flags,
                expr: _,
                delim,
            } => {
                if !flags.contains('r') {
                    flags.push('r');
                }
                ExprKind::Substitution {
                    expr: Box::new(lhs),
                    pattern,
                    replacement,
                    flags,
                    delim,
                }
            }
            ExprKind::Transliterate {
                from,
                to,
                mut flags,
                expr: _,
                delim,
            } => {
                if !flags.contains('r') {
                    flags.push('r');
                }
                ExprKind::Transliterate {
                    expr: Box::new(lhs),
                    from,
                    to,
                    flags,
                    delim,
                }
            }
            ExprKind::Match {
                pattern,
                flags,
                scalar_g,
                expr: _,
                delim,
            } => ExprKind::Match {
                expr: Box::new(lhs),
                pattern,
                flags,
                scalar_g,
                delim,
            },
            // Bare `/regex/` (no explicit `m`): promote to Match on piped LHS
            ExprKind::Regex(pattern, flags) => ExprKind::Match {
                expr: Box::new(lhs),
                pattern,
                flags,
                scalar_g: false,
                delim: '/',
            },

            // ── Bareword function name → plain unary call ──────────────────────
            ExprKind::Bareword(name) => match name.as_str() {
                "reverse" => {
                    if !crate::compat_mode() {
                        return Err(self
                            .syntax_err("`reverse` is not valid stryke; use `rev` instead", line));
                    }
                    ExprKind::ReverseExpr(Box::new(lhs))
                }
                "rv" | "reversed" | "rev" => ExprKind::Rev(Box::new(lhs)),
                "uq" | "uniq" | "distinct" => ExprKind::FuncCall {
                    name: "uniq".to_string(),
                    args: vec![lhs],
                },
                "fl" | "flatten" => ExprKind::FuncCall {
                    name: "flatten".to_string(),
                    args: vec![lhs],
                },
                _ => ExprKind::FuncCall {
                    name,
                    args: vec![lhs],
                },
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

            // `LHS |> >{ BLOCK }` — the `>{}` form is parsed everywhere as `Do(CodeRef)` (IIFE).
            // On the RHS of `|>` we want pipe-apply semantics instead: unwrap the Do and invoke
            // the inner coderef with `lhs` as `$_[0]`, matching `LHS |> sub { ... }`.
            ExprKind::Do(inner) if matches!(inner.kind, ExprKind::CodeRef { .. }) => {
                ExprKind::IndirectCall {
                    target: inner,
                    args: vec![lhs],
                    ampersand: false,
                    pass_caller_arglist: false,
                }
            }

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
        let left = self.parse_shift()?;
        let first_op = match self.peek() {
            Token::NumLt => BinOp::NumLt,
            Token::NumGt => BinOp::NumGt,
            Token::NumLe => BinOp::NumLe,
            Token::NumGe => BinOp::NumGe,
            Token::StrLt => BinOp::StrLt,
            Token::StrGt => BinOp::StrGt,
            Token::StrLe => BinOp::StrLe,
            Token::StrGe => BinOp::StrGe,
            _ => return Ok(left),
        };
        let line = left.line;
        self.advance();
        let middle = self.parse_shift()?;

        let second_op = match self.peek() {
            Token::NumLt => Some(BinOp::NumLt),
            Token::NumGt => Some(BinOp::NumGt),
            Token::NumLe => Some(BinOp::NumLe),
            Token::NumGe => Some(BinOp::NumGe),
            Token::StrLt => Some(BinOp::StrLt),
            Token::StrGt => Some(BinOp::StrGt),
            Token::StrLe => Some(BinOp::StrLe),
            Token::StrGe => Some(BinOp::StrGe),
            _ => None,
        };

        if second_op.is_none() {
            return Ok(Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(left),
                    op: first_op,
                    right: Box::new(middle),
                },
                line,
            });
        }

        // Chained comparison: `a < b < c` → `(a < b) && (b < c)`
        // Collect all operands and operators for chains like `1 < x < 10 < y`
        let mut operands = vec![left, middle];
        let mut ops = vec![first_op];

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
            self.advance();
            ops.push(op);
            operands.push(self.parse_shift()?);
        }

        // Build `(a op0 b) && (b op1 c) && (c op2 d) && ...`
        let mut result = Expr {
            kind: ExprKind::BinOp {
                left: Box::new(operands[0].clone()),
                op: ops[0],
                right: Box::new(operands[1].clone()),
            },
            line,
        };

        for i in 1..ops.len() {
            let cmp = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(operands[i].clone()),
                    op: ops[i],
                    right: Box::new(operands[i + 1].clone()),
                },
                line,
            };
            result = Expr {
                kind: ExprKind::BinOp {
                    left: Box::new(result),
                    op: BinOp::LogAnd,
                    right: Box::new(cmp),
                },
                line,
            };
        }

        Ok(result)
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
            // Implicit semicolon: `-` or `+` on a new line is a unary operator on
            // the next statement, not a binary operator continuing this expression.
            let op = match self.peek() {
                Token::Plus if self.peek_line() == self.prev_line() => BinOp::Add,
                Token::Minus if self.peek_line() == self.prev_line() => BinOp::Sub,
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
                Token::Slash if self.suppress_slash_as_div == 0 => BinOp::Div,
                // Implicit semicolon: `%` on a new line is a hash dereference or hash
                // sigil for the next statement, not modulo operator on this expression.
                Token::Percent if self.peek_line() == self.prev_line() => BinOp::Mod,
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
                    Token::Regex(pattern, flags, delim) => {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Match {
                                expr: Box::new(left),
                                pattern,
                                flags,
                                scalar_g: false,
                                delim,
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
                            let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                            Ok(Expr {
                                kind: ExprKind::Substitution {
                                    expr: Box::new(left),
                                    pattern: parts[2].to_string(),
                                    replacement: parts[3].to_string(),
                                    flags: parts.get(4).unwrap_or(&"").to_string(),
                                    delim,
                                },
                                line,
                            })
                        } else if parts.len() >= 4 && parts[1] == "tr" {
                            let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                            Ok(Expr {
                                kind: ExprKind::Transliterate {
                                    expr: Box::new(left),
                                    from: parts[2].to_string(),
                                    to: parts[3].to_string(),
                                    flags: parts.get(4).unwrap_or(&"").to_string(),
                                    delim,
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
                    Token::Regex(pattern, flags, delim) => {
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
                                        delim,
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
                            let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                            Ok(Expr {
                                kind: ExprKind::UnaryOp {
                                    op: UnaryOp::LogNot,
                                    expr: Box::new(Expr {
                                        kind: ExprKind::Substitution {
                                            expr: Box::new(left),
                                            pattern: parts[2].to_string(),
                                            replacement: parts[3].to_string(),
                                            flags: parts.get(4).unwrap_or(&"").to_string(),
                                            delim,
                                        },
                                        line,
                                    }),
                                },
                                line,
                            })
                        } else if parts.len() >= 4 && parts[1] == "tr" {
                            let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                            Ok(Expr {
                                kind: ExprKind::UnaryOp {
                                    op: UnaryOp::LogNot,
                                    expr: Box::new(Expr {
                                        kind: ExprKind::Transliterate {
                                            expr: Box::new(left),
                                            from: parts[2].to_string(),
                                            to: parts[3].to_string(),
                                            flags: parts.get(4).unwrap_or(&"").to_string(),
                                            delim,
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

    /// Parse thread macro input. Like `parse_range` but suppresses `/` as division
    /// so that `/pattern/` is left for the thread stage parser to handle as regex filter.
    fn parse_thread_input(&mut self) -> PerlResult<Expr> {
        self.suppress_slash_as_div = self.suppress_slash_as_div.saturating_add(1);
        let result = self.parse_range();
        self.suppress_slash_as_div = self.suppress_slash_as_div.saturating_sub(1);
        result
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
                    // Implicit semicolon: `++` on a new line is a prefix operator
                    // on the next statement, not postfix on the previous expression.
                    if self.peek_line() > self.prev_line() {
                        break;
                    }
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
                    // Implicit semicolon: `--` on a new line is a prefix operator
                    // on the next statement, not postfix on the previous expression.
                    if self.peek_line() > self.prev_line() {
                        break;
                    }
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
                    // Implicit semicolon: `(` on a new line after an expression
                    // is a new statement, not a postfix code-ref call.
                    // e.g.  `my $x = $ENV{"KEY"}\n($y =~ s/.../.../)`
                    if self.peek_line() > self.prev_line() {
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
                            let key = self.parse_hash_subscript_key()?;
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
                        // Postfix dereference (Perl 5.20+, default 5.24+):
                        //   `$ref->@*`         — full array      ≡ `@{$ref}`
                        //   `$ref->@[i,j]`     — array slice     ≡ `@{$ref}[i,j]`
                        //   `$ref->@{k,l}`     — hash slice (vals) ≡ `@{$ref}{k,l}`
                        //   `$ref->%*`         — full hash       ≡ `%{$ref}`
                        Token::ArrayAt => {
                            self.advance(); // consume `@`
                            match self.peek().clone() {
                                Token::Star => {
                                    self.advance();
                                    expr = Expr {
                                        kind: ExprKind::Deref {
                                            expr: Box::new(expr),
                                            kind: Sigil::Array,
                                        },
                                        line,
                                    };
                                }
                                Token::LBracket => {
                                    self.advance();
                                    let mut indices = Vec::new();
                                    while !matches!(self.peek(), Token::RBracket | Token::Eof) {
                                        indices.push(self.parse_assign_expr()?);
                                        if !self.eat(&Token::Comma) {
                                            break;
                                        }
                                    }
                                    self.expect(&Token::RBracket)?;
                                    let source = Expr {
                                        kind: ExprKind::Deref {
                                            expr: Box::new(expr),
                                            kind: Sigil::Array,
                                        },
                                        line,
                                    };
                                    expr = Expr {
                                        kind: ExprKind::AnonymousListSlice {
                                            source: Box::new(source),
                                            indices,
                                        },
                                        line,
                                    };
                                }
                                Token::LBrace => {
                                    self.advance();
                                    let mut keys = Vec::new();
                                    while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                                        keys.push(self.parse_assign_expr()?);
                                        if !self.eat(&Token::Comma) {
                                            break;
                                        }
                                    }
                                    self.expect(&Token::RBrace)?;
                                    expr = Expr {
                                        kind: ExprKind::HashSliceDeref {
                                            container: Box::new(expr),
                                            keys,
                                        },
                                        line,
                                    };
                                }
                                tok => {
                                    return Err(self.syntax_err(
                                        format!(
                                            "Expected `*`, `[…]`, or `{{…}}` after `->@`, got {:?}",
                                            tok
                                        ),
                                        line,
                                    ));
                                }
                            }
                        }
                        Token::HashPercent => {
                            self.advance(); // consume `%`
                            match self.peek().clone() {
                                Token::Star => {
                                    self.advance();
                                    expr = Expr {
                                        kind: ExprKind::Deref {
                                            expr: Box::new(expr),
                                            kind: Sigil::Hash,
                                        },
                                        line,
                                    };
                                }
                                tok => {
                                    return Err(self.syntax_err(
                                        format!("Expected `*` after `->%`, got {:?}", tok),
                                        line,
                                    ));
                                }
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
                    // Implicit semicolon: `{` on a new line is a new statement (block/hashref),
                    // not a hash subscript on the preceding expression.
                    if self.peek_line() > self.prev_line() {
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
                    let key = self.parse_hash_subscript_key()?;
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
        // `my $x = …` (or `our` / `state` / `local`) used inside an expression —
        // typically `if (my $x = …)` / `while (my $line = <FH>)`.  Returns the
        // assigned value(s); has the side effect of declaring the variable in
        // the current scope.  See `ExprKind::MyExpr`.
        if let Token::Ident(ref kw) = self.peek().clone() {
            if matches!(kw.as_str(), "my" | "our" | "state" | "local") {
                let kw_owned = kw.clone();
                // Parse exactly like the statement form via `parse_my_our_local`,
                // then unwrap the resulting `StmtKind::*` back into a list of
                // `VarDecl`s for the expression node.  This re-uses the full
                // syntax (typed sigs, list destructuring, type annotations).
                let saved_pos = self.pos;
                let stmt = self.parse_my_our_local(&kw_owned, false)?;
                let decls = match stmt.kind {
                    StmtKind::My(d)
                    | StmtKind::Our(d)
                    | StmtKind::State(d)
                    | StmtKind::Local(d) => d,
                    _ => {
                        // `local *FOO = …` / non-decl forms — fall back to the
                        // statement parser (already advanced); restore position
                        // and let the surrounding code handle it as a statement
                        // by erroring loudly here.
                        self.pos = saved_pos;
                        return Err(self.syntax_err(
                            "`my`/`our`/`local` in expression must declare variables",
                            line,
                        ));
                    }
                };
                return Ok(Expr {
                    kind: ExprKind::MyExpr {
                        keyword: kw_owned,
                        decls,
                    },
                    line,
                });
            }
        }
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
            // `>{ BLOCK }` — IIFE block expression (immediately-invoked anonymous sub).
            // Valid in any expression position; evaluates the block and yields its last value.
            // In thread-macro stage position (`EXPR |>` already consumed by the stage loop in
            // `parse_thread_macro`), the explicit branch at ~1417 wins and the block is
            // instead pipe-applied as a coderef — that path is never reached from here.
            Token::ArrowBrace => {
                self.advance();
                let mut stmts = Vec::new();
                while !matches!(self.peek(), Token::RBrace | Token::Eof) {
                    if self.eat(&Token::Semicolon) {
                        continue;
                    }
                    stmts.push(self.parse_statement()?);
                }
                self.expect(&Token::RBrace)?;
                let inner_line = stmts.first().map(|s| s.line).unwrap_or(line);
                let inner = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: stmts,
                    },
                    line: inner_line,
                };
                Ok(Expr {
                    kind: ExprKind::Do(Box::new(inner)),
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
            Token::HereDoc(_, body, interpolate) => {
                self.advance();
                if interpolate {
                    self.parse_interpolated_string(&body, line)
                } else {
                    Ok(Expr {
                        kind: ExprKind::String(body),
                        line,
                    })
                }
            }
            Token::Regex(pattern, flags, _delim) => {
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
                // `%[a => 1, b => 2]` — sugar for `%{+{a=>1,b=>2}}`: dereference an
                // anonymous hashref inline, using `[...]` as the delimiter to avoid
                // the block-vs-hashref ambiguity that `%{a=>1}` has in real Perl.
                // Real Perl errors on `%[...]` syntactically, so no compat risk.
                if matches!(self.peek(), Token::LBracket) {
                    self.advance();
                    let pairs = self.parse_hashref_pairs_until(&Token::RBracket)?;
                    self.expect(&Token::RBracket)?;
                    let href = Expr {
                        kind: ExprKind::HashRef(pairs),
                        line,
                    };
                    return Ok(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(href),
                            kind: Sigil::Hash,
                        },
                        line,
                    });
                }
                self.expect(&Token::LBrace)?;
                // Peek to disambiguate `%{ $ref }` (deref a hashref expression) from
                // `%{ k => v }` (inline hash literal). Real Perl's block-vs-hashref
                // heuristic is famously unreliable — when the first non-whitespace
                // token is an ident/string followed by `=>`, treat the whole thing
                // as a hashref literal to make `%{a=>1,b=>2}` work reliably.
                let looks_like_pair = matches!(
                    self.peek(),
                    Token::Ident(_) | Token::SingleString(_) | Token::DoubleString(_)
                ) && matches!(self.peek_at(1), Token::FatArrow);
                let inner = if looks_like_pair {
                    let pairs = self.parse_hashref_pairs_until(&Token::RBrace)?;
                    Expr {
                        kind: ExprKind::HashRef(pairs),
                        line,
                    }
                } else {
                    self.parse_expression()?
                };
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
                // `@[a, b, c]` — sugar for `@{[a, b, c]}`: dereference an
                // anonymous arrayref inline. Real Perl rejects `@[...]` at
                // the parser level, so this extension has no compat risk.
                if matches!(self.peek(), Token::LBracket) {
                    self.advance();
                    let mut elems = Vec::new();
                    if !matches!(self.peek(), Token::RBracket) {
                        elems.push(self.parse_assign_expr()?);
                        while self.eat(&Token::Comma) {
                            if matches!(self.peek(), Token::RBracket) {
                                break;
                            }
                            elems.push(self.parse_assign_expr()?);
                        }
                    }
                    self.expect(&Token::RBracket)?;
                    let aref = Expr {
                        kind: ExprKind::ArrayRef(elems),
                        line,
                    };
                    return Ok(Expr {
                        kind: ExprKind::Deref {
                            expr: Box::new(aref),
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
                            "Expected `$name`, `{`, or `[` after `@` (e.g. `@$aref`, `@{expr}`, `@[1,2,3]`, or `@$href{keys}`)",
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
            Token::ThreadArrow => {
                self.advance();
                self.parse_thread_macro(line)
            }
            Token::Ident(ref name) => {
                let name = name.clone();
                // Handle s///
                if name.starts_with('\x00') {
                    self.advance();
                    let parts: Vec<&str> = name.split('\x00').collect();
                    if parts.len() >= 4 && parts[1] == "s" {
                        let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                        return Ok(Expr {
                            kind: ExprKind::Substitution {
                                expr: Box::new(Expr {
                                    kind: ExprKind::ScalarVar("_".into()),
                                    line,
                                }),
                                pattern: parts[2].to_string(),
                                replacement: parts[3].to_string(),
                                flags: parts.get(4).unwrap_or(&"").to_string(),
                                delim,
                            },
                            line,
                        });
                    }
                    if parts.len() >= 4 && parts[1] == "tr" {
                        let delim = parts.get(5).and_then(|s| s.chars().next()).unwrap_or('/');
                        return Ok(Expr {
                            kind: ExprKind::Transliterate {
                                expr: Box::new(Expr {
                                    kind: ExprKind::ScalarVar("_".into()),
                                    line,
                                }),
                                from: parts[2].to_string(),
                                to: parts[3].to_string(),
                                flags: parts.get(4).unwrap_or(&"").to_string(),
                                delim,
                            },
                            line,
                        });
                    }
                    return Err(self.syntax_err("Unexpected encoded token", line));
                }
                self.parse_named_expr(name)
            }

            // `%name` when lexer emitted `Token::Percent` (due to preceding term context)
            // instead of `Token::HashVar`. This happens after `t` (thread macro) etc.
            Token::Percent => {
                self.advance();
                match self.peek().clone() {
                    Token::Ident(name) => {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::HashVar(name),
                            line,
                        })
                    }
                    Token::ScalarVar(n) => {
                        self.advance();
                        Ok(Expr {
                            kind: ExprKind::Deref {
                                expr: Box::new(Expr {
                                    kind: ExprKind::ScalarVar(n),
                                    line,
                                }),
                                kind: Sigil::Hash,
                            },
                            line,
                        })
                    }
                    Token::LBrace => {
                        self.advance();
                        let looks_like_pair = matches!(
                            self.peek(),
                            Token::Ident(_) | Token::SingleString(_) | Token::DoubleString(_)
                        ) && matches!(self.peek_at(1), Token::FatArrow);
                        let inner = if looks_like_pair {
                            let pairs = self.parse_hashref_pairs_until(&Token::RBrace)?;
                            Expr {
                                kind: ExprKind::HashRef(pairs),
                                line,
                            }
                        } else {
                            self.parse_expression()?
                        };
                        self.expect(&Token::RBrace)?;
                        Ok(Expr {
                            kind: ExprKind::Deref {
                                expr: Box::new(inner),
                                kind: Sigil::Hash,
                            },
                            line,
                        })
                    }
                    Token::LBracket => {
                        self.advance();
                        let pairs = self.parse_hashref_pairs_until(&Token::RBracket)?;
                        self.expect(&Token::RBracket)?;
                        let href = Expr {
                            kind: ExprKind::HashRef(pairs),
                            line,
                        };
                        Ok(Expr {
                            kind: ExprKind::Deref {
                                expr: Box::new(href),
                                kind: Sigil::Hash,
                            },
                            line,
                        })
                    }
                    tok => Err(self.syntax_err(
                        format!(
                            "Expected identifier, `$`, `{{`, or `[` after `%`, got {:?}",
                            tok
                        ),
                        line,
                    )),
                }
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

        // Fat-arrow auto-quoting: ANY bareword (including keywords/builtins)
        // before `=>` is treated as a string key, matching Perl 5 semantics.
        // e.g. `(print => 1, pr => "x", sort => 3)` are all valid hash pairs.
        if matches!(self.peek(), Token::FatArrow) {
            return Ok(Expr {
                kind: ExprKind::String(name),
                line,
            });
        }

        if crate::compat_mode() {
            if let Some(ext) = Self::stryke_extension_name(&name) {
                if !self.declared_subs.contains(&name) {
                    return Err(self.syntax_err(
                        format!("`{ext}` is a stryke extension (disabled by --compat)"),
                        line,
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
            "__SUB__" => Ok(Expr {
                kind: ExprKind::MagicConst(MagicConstKind::Sub),
                line,
            }),
            "stdin" => Ok(Expr {
                kind: ExprKind::FuncCall {
                    name: "stdin".into(),
                    args: vec![],
                },
                line,
            }),
            "range" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "range".into(),
                        args,
                    },
                    line,
                })
            }
            "print" | "pr" => self.parse_print_like(|h, a| ExprKind::Print { handle: h, args: a }),
            "say" => {
                if !crate::compat_mode() {
                    return Err(self.syntax_err("`say` is not valid stryke; use `p` instead", line));
                }
                self.parse_print_like(|h, a| ExprKind::Say { handle: h, args: a })
            }
            "p" => self.parse_print_like(|h, a| ExprKind::Say { handle: h, args: a }),
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
            // `croak` / `confess` — `Carp` builtins available without `use Carp`
            // (matches the doc claim in `lsp.rs:1243`). For now both desugar to
            // `die` — TODO: croak should report caller's file/line, confess
            // should append a full stack trace.
            "croak" | "confess" => {
                let args = self.parse_list_until_terminator()?;
                Ok(Expr {
                    kind: ExprKind::Die(args),
                    line,
                })
            }
            // `carp` / `cluck` — `Carp` warning siblings of `croak`/`confess`.
            "carp" | "cluck" => {
                let args = self.parse_list_until_terminator()?;
                Ok(Expr {
                    kind: ExprKind::Warn(args),
                    line,
                })
            }
            "chomp" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chomp(Box::new(a)),
                    line,
                })
            }
            "chop" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chop(Box::new(a)),
                    line,
                })
            }
            "length" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Length(Box::new(a)),
                    line,
                })
            }
            "defined" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Defined(Box::new(a)),
                    line,
                })
            }
            "ref" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Ref(Box::new(a)),
                    line,
                })
            }
            "undef" => {
                // `undef $var` sets `$var` to undef — but a variable on a new line
                // is a separate statement (implicit semicolon), not an argument.
                if self.peek_line() == self.prev_line()
                    && matches!(
                        self.peek(),
                        Token::ScalarVar(_) | Token::ArrayVar(_) | Token::HashVar(_)
                    )
                {
                    let target = self.parse_primary()?;
                    return Ok(Expr {
                        kind: ExprKind::Assign {
                            target: Box::new(target),
                            value: Box::new(Expr {
                                kind: ExprKind::Undef,
                                line,
                            }),
                        },
                        line,
                    });
                }
                Ok(Expr {
                    kind: ExprKind::Undef,
                    line,
                })
            }
            "scalar" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::ScalarContext(Box::new(a)),
                    line,
                })
            }
            "abs" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Abs(Box::new(a)),
                    line,
                })
            }
            // stryke unary numeric extensions — treat like `abs` so a bare
            // identifier in `map { inc }` / `for (…) { p inc }` becomes a
            // call with implicit `$_` rather than falling through to the
            // generic `Bareword` arm (which stringifies to `"inc"`).
            "inc" | "dec" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name,
                        args: vec![a],
                    },
                    line,
                })
            }
            "int" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Int(Box::new(a)),
                    line,
                })
            }
            "sqrt" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Sqrt(Box::new(a)),
                    line,
                })
            }
            "sin" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Sin(Box::new(a)),
                    line,
                })
            }
            "cos" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Cos(Box::new(a)),
                    line,
                })
            }
            "atan2" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Exp(Box::new(a)),
                    line,
                })
            }
            "log" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Log(Box::new(a)),
                    line,
                })
            }
            "input" => {
                let args = if matches!(
                    self.peek(),
                    Token::Semicolon
                        | Token::RBrace
                        | Token::RParen
                        | Token::Eof
                        | Token::Comma
                        | Token::PipeForward
                ) {
                    vec![]
                } else if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        vec![]
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        vec![a]
                    }
                } else {
                    let a = self.parse_one_arg()?;
                    vec![a]
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "input".to_string(),
                        args,
                    },
                    line,
                })
            }
            "rand" => {
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
                    Token::Semicolon
                        | Token::RBrace
                        | Token::RParen
                        | Token::Eof
                        | Token::Comma
                        | Token::PipeForward
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Hex(Box::new(a)),
                    line,
                })
            }
            "oct" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Oct(Box::new(a)),
                    line,
                })
            }
            "chr" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chr(Box::new(a)),
                    line,
                })
            }
            "ord" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Ord(Box::new(a)),
                    line,
                })
            }
            "lc" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Lc(Box::new(a)),
                    line,
                })
            }
            "uc" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Uc(Box::new(a)),
                    line,
                })
            }
            "lcfirst" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Lcfirst(Box::new(a)),
                    line,
                })
            }
            "ucfirst" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Ucfirst(Box::new(a)),
                    line,
                })
            }
            "fc" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Fc(Box::new(a)),
                    line,
                })
            }
            "crypt" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                    Token::Semicolon
                        | Token::RBrace
                        | Token::RParen
                        | Token::Eof
                        | Token::Comma
                        | Token::PipeForward
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Study(Box::new(a)),
                    line,
                })
            }
            "push" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_argv()?;
                Ok(Expr {
                    kind: ExprKind::Pop(Box::new(a)),
                    line,
                })
            }
            "shift" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_argv()?;
                Ok(Expr {
                    kind: ExprKind::Shift(Box::new(a)),
                    line,
                })
            }
            "unshift" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_postfix()?;
                Ok(Expr {
                    kind: ExprKind::Delete(Box::new(a)),
                    line,
                })
            }
            "exists" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_postfix()?;
                Ok(Expr {
                    kind: ExprKind::Exists(Box::new(a)),
                    line,
                })
            }
            "keys" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Keys(Box::new(a)),
                    line,
                })
            }
            "values" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Values(Box::new(a)),
                    line,
                })
            }
            "each" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Each(Box::new(a)),
                    line,
                })
            }
            "fore" | "e" | "ep" => {
                // `fore { BLOCK } LIST` / `ep` — forEach expression (pipe-forward friendly)
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
                    // `|> ep` — bare ep at end of pipe: default to `say $_`
                    // `|> fore say` / `|> e say` — blockless pipe form: wrap EXPR into a synthetic block
                    let is_terminal = matches!(
                        self.peek(),
                        Token::Semicolon
                            | Token::RParen
                            | Token::Eof
                            | Token::PipeForward
                            | Token::RBrace
                    );
                    let block = if name == "ep" && is_terminal {
                        vec![Statement {
                            label: None,
                            kind: StmtKind::Expression(Expr {
                                kind: ExprKind::Say {
                                    handle: None,
                                    args: vec![Expr {
                                        kind: ExprKind::ScalarVar("_".into()),
                                        line,
                                    }],
                                },
                                line,
                            }),
                            line,
                        }]
                    } else {
                        let expr = self.parse_assign_expr_stop_at_pipe()?;
                        let expr = Self::lift_bareword_to_topic_call(expr);
                        vec![Statement {
                            label: None,
                            kind: StmtKind::Expression(expr),
                            line,
                        }]
                    };
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
            "rev" => {
                // `rev` — context-aware reverse: string in scalar, list in list context.
                // Defaults to $_ when no argument given.
                // Only use pipe placeholder when directly in pipe RHS (not inside a block).
                // RBrace means we're inside a block like `map { rev }` - use $_ default.
                let a = if self.in_pipe_rhs()
                    && matches!(
                        self.peek(),
                        Token::Semicolon | Token::RParen | Token::Eof | Token::PipeForward
                    ) {
                    self.pipe_placeholder_list(line)
                } else {
                    self.parse_one_arg_or_default()?
                };
                Ok(Expr {
                    kind: ExprKind::Rev(Box::new(a)),
                    line,
                })
            }
            "reverse" => {
                if !crate::compat_mode() {
                    return Err(
                        self.syntax_err("`reverse` is not valid stryke; use `rev` instead", line)
                    );
                }
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
            "reversed" | "rv" => {
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
                    kind: ExprKind::Rev(Box::new(a)),
                    line,
                })
            }
            "join" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
            "map" | "flat_map" | "maps" | "flat_maps" => {
                let flatten_array_refs = matches!(name.as_str(), "flat_map" | "flat_maps");
                let stream = matches!(name.as_str(), "maps" | "flat_maps");
                if matches!(self.peek(), Token::LBrace) {
                    let (block, list) = self.parse_block_list()?;
                    Ok(Expr {
                        kind: ExprKind::MapExpr {
                            block,
                            list: Box::new(list),
                            flatten_array_refs,
                            stream,
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
                            stream,
                        },
                        line,
                    })
                }
            }
            "match" => {
                if crate::compat_mode() {
                    return Err(self.syntax_err(
                        "algebraic `match` is a stryke extension (disabled by --compat)",
                        line,
                    ));
                }
                self.parse_algebraic_match_expr(line)
            }
            "grep" | "greps" | "filter" | "fi" | "find_all" => {
                let keyword = match name.as_str() {
                    "grep" => crate::ast::GrepBuiltinKeyword::Grep,
                    "greps" => crate::ast::GrepBuiltinKeyword::Greps,
                    "filter" | "fi" => crate::ast::GrepBuiltinKeyword::Filter,
                    "find_all" => crate::ast::GrepBuiltinKeyword::FindAll,
                    _ => unreachable!(),
                };
                if matches!(self.peek(), Token::LBrace) {
                    let (block, list) = self.parse_block_list()?;
                    Ok(Expr {
                        kind: ExprKind::GrepExpr {
                            block,
                            list: Box::new(list),
                            keyword,
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
                                keyword,
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
                                keyword,
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
                } else if matches!(self.peek(), Token::Ident(ref name) if !Self::is_known_bareword(name))
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
                        stream: false,
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
                        stream: false,
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
                        stream: false,
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
                        stream: false,
                    },
                    line,
                })
            }
            "pmaps" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        flat_outputs: false,
                        on_cluster: None,
                        stream: true,
                    },
                    line,
                })
            }
            "pflat_maps" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PMapExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        flat_outputs: true,
                        on_cluster: None,
                        stream: true,
                    },
                    line,
                })
            }
            "pgreps" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::PGrepExpr {
                        block,
                        list: Box::new(list),
                        progress: progress.map(Box::new),
                        stream: true,
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
                        stream: false,
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
            "par_lines" | "par_walk" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(
                        self.syntax_err(format!("{} requires at least two arguments", name), line)
                    );
                }

                if name == "par_lines" {
                    Ok(Expr {
                        kind: ExprKind::ParLinesExpr {
                            path: Box::new(args[0].clone()),
                            callback: Box::new(args[1].clone()),
                            progress: None,
                        },
                        line,
                    })
                } else {
                    Ok(Expr {
                        kind: ExprKind::ParWalkExpr {
                            path: Box::new(args[0].clone()),
                            callback: Box::new(args[1].clone()),
                            progress: None,
                        },
                        line,
                    })
                }
            }
            "pwatch" | "watch" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(
                        self.syntax_err(format!("{} requires at least two arguments", name), line)
                    );
                }
                Ok(Expr {
                    kind: ExprKind::PwatchExpr {
                        path: Box::new(args[0].clone()),
                        callback: Box::new(args[1].clone()),
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
            "spinner" => {
                // `spinner "msg" { BLOCK }` or `spinner { BLOCK }`
                let (message, body) = if matches!(self.peek(), Token::LBrace) {
                    let body = self.parse_block()?;
                    (
                        Box::new(Expr {
                            kind: ExprKind::String("working".to_string()),
                            line,
                        }),
                        body,
                    )
                } else {
                    let msg = self.parse_assign_expr()?;
                    let body = self.parse_block()?;
                    (Box::new(msg), body)
                };
                Ok(Expr {
                    kind: ExprKind::Spinner { message, body },
                    line,
                })
            }
            "thread" | "t" => {
                // `thread EXPR stage1 stage2 ...` — threading macro (like Clojure's ->>)
                // `t` is a short alias for `thread`
                // Each stage is either:
                //   - `ident` — bare function call
                //   - `ident { block }` — function with block arg
                //   - `ident arg1 arg2 { block }` — function with args and optional block
                //   - `sub { block }` — standalone anonymous block
                //   - `>{ block }` — shorthand for standalone anonymous block
                // Desugars to: EXPR |> stage1 |> stage2 |> ...
                self.parse_thread_macro(line)
            }
            "retry" => {
                // `retry { BLOCK }` or `retry BAREWORD` — bareword becomes zero-arg call.
                // An optional comma before `times` is allowed in both forms.
                let body = if matches!(self.peek(), Token::LBrace) {
                    self.parse_block()?
                } else {
                    let bw_line = self.peek_line();
                    let Token::Ident(ref name) = self.peek().clone() else {
                        return Err(self
                            .syntax_err("retry: expected block or bareword function name", line));
                    };
                    let name = name.clone();
                    self.advance();
                    vec![Statement::new(
                        StmtKind::Expression(Expr {
                            kind: ExprKind::FuncCall { name, args: vec![] },
                            line: bw_line,
                        }),
                        bw_line,
                    )]
                };
                self.eat(&Token::Comma);
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
                let max = Box::new(self.parse_assign_expr()?);
                self.expect(&Token::Comma)?;
                let window = Box::new(self.parse_assign_expr()?);
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
                // `every("500ms") { BLOCK }` or `every "500ms" BODY` — parens optional.
                // Body consumes `|>` (every is an infinite loop, not a pipeable source).
                let has_paren = self.eat(&Token::LParen);
                let interval = Box::new(self.parse_assign_expr()?);
                if has_paren {
                    self.expect(&Token::RParen)?;
                }
                let body = if matches!(self.peek(), Token::LBrace) {
                    self.parse_block()?
                } else {
                    let bline = self.peek_line();
                    let expr = self.parse_assign_expr()?;
                    vec![Statement::new(StmtKind::Expression(expr), bline)]
                };
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                // `await` defaults to `$_` so `map { await } @tasks` works
                // (Perl-style topic-defaulting unary).
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Await(Box::new(a)),
                    line,
                })
            }
            "slurp" | "cat" | "c" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Slurp(Box::new(a)),
                    line,
                })
            }
            "capture" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Capture(Box::new(a)),
                    line,
                })
            }
            "fetch_url" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                    || matches!(self.peek(), Token::Ident(ref name) if !Self::is_known_bareword(name))
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
            // `size` is the file-size builtin (Perl `-s`), not a list-count alias.
            // Defaults to `$_` when no arg is given, like `length`. See
            // `builtin_file_size` in builtins.rs for the runtime behavior.
            "size" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                if self.pipe_supplies_slurped_list_operand() {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: "size".to_string(),
                            args: vec![],
                        },
                        line,
                    });
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "size".to_string(),
                        args: vec![a],
                    },
                    line,
                })
            }
            "list_count" | "list_size" | "count" | "len" | "cnt" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                        "`progress =>` is not supported for list_count / list_size / count / cnt",
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
            "shuffle" | "shuffled" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
            "take_while" | "drop_while" | "skip_while" | "reject" | "tap" | "peek"
            | "partition" | "min_by" | "max_by" | "zip_with" | "count_by" => {
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                if progress.is_some() {
                    return Err(
                        self.syntax_err(format!("`progress =>` is not supported for {name}"), line)
                    );
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Close(Box::new(a)),
                    line,
                })
            }
            "opendir" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Readdir(Box::new(a)),
                    line,
                })
            }
            "closedir" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Closedir(Box::new(a)),
                    line,
                })
            }
            "rewinddir" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Rewinddir(Box::new(a)),
                    line,
                })
            }
            "telldir" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Telldir(Box::new(a)),
                    line,
                })
            }
            "seekdir" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::System(args),
                    line,
                })
            }
            "exec" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Exec(args),
                    line,
                })
            }
            "eval" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                    self.parse_one_arg_or_default()?
                };
                Ok(Expr {
                    kind: ExprKind::Eval(Box::new(a)),
                    line,
                })
            }
            "do" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Do(Box::new(a)),
                    line,
                })
            }
            "require" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg()?;
                Ok(Expr {
                    kind: ExprKind::Require(Box::new(a)),
                    line,
                })
            }
            "exit" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                if matches!(
                    self.peek(),
                    Token::Semicolon | Token::RBrace | Token::Eof | Token::PipeForward
                ) {
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Chdir(Box::new(a)),
                    line,
                })
            }
            "mkdir" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Unlink(args),
                    line,
                })
            }
            "rename" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                let arg = if args.len() == 1 {
                    args[0].clone()
                } else if args.is_empty() {
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line,
                    }
                } else {
                    return Err(self.syntax_err("stat requires zero or one argument", line));
                };
                Ok(Expr {
                    kind: ExprKind::Stat(Box::new(arg)),
                    line,
                })
            }
            "lstat" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                let arg = if args.len() == 1 {
                    args[0].clone()
                } else if args.is_empty() {
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line,
                    }
                } else {
                    return Err(self.syntax_err("lstat requires zero or one argument", line));
                };
                Ok(Expr {
                    kind: ExprKind::Lstat(Box::new(arg)),
                    line,
                })
            }
            "link" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                let arg = if args.len() == 1 {
                    args[0].clone()
                } else if args.is_empty() {
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line,
                    }
                } else {
                    return Err(self.syntax_err("readlink requires zero or one argument", line));
                };
                Ok(Expr {
                    kind: ExprKind::Readlink(Box::new(arg)),
                    line,
                })
            }
            "files" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Files(args),
                    line,
                })
            }
            "filesf" | "f" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Filesf(args),
                    line,
                })
            }
            "fr" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::FilesfRecursive(args),
                    line,
                })
            }
            "dirs" | "d" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Dirs(args),
                    line,
                })
            }
            "dr" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::DirsRecursive(args),
                    line,
                })
            }
            "sym_links" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::SymLinks(args),
                    line,
                })
            }
            "sockets" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Sockets(args),
                    line,
                })
            }
            "pipes" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Pipes(args),
                    line,
                })
            }
            "block_devices" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::BlockDevices(args),
                    line,
                })
            }
            "char_devices" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::CharDevices(args),
                    line,
                })
            }
            "glob" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Glob(args),
                    line,
                })
            }
            "glob_par" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let (args, progress) = self.parse_glob_par_or_par_sed_args()?;
                Ok(Expr {
                    kind: ExprKind::GlobPar { args, progress },
                    line,
                })
            }
            "par_sed" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let (args, progress) = self.parse_glob_par_or_par_sed_args()?;
                Ok(Expr {
                    kind: ExprKind::ParSed { args, progress },
                    line,
                })
            }
            "bless" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
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
            "wantarray" => {
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    self.expect(&Token::RParen)?;
                }
                Ok(Expr {
                    kind: ExprKind::Wantarray,
                    line,
                })
            }
            "sub" => {
                // In non-compat mode, `sub {}` is not valid — must use `fn {}`
                if !crate::compat_mode() {
                    return Err(self.syntax_err(
                        "`sub {}` anonymous subroutine is not valid stryke; use `fn {}` instead",
                        line,
                    ));
                }
                // Anonymous sub — optional prototype `sub () { }` (e.g. Carp.pm `*X = sub () { 1 }`)
                let (params, _prototype) = self.parse_sub_sig_or_prototype_opt()?;
                let body = self.parse_block()?;
                Ok(Expr {
                    kind: ExprKind::CodeRef { params, body },
                    line,
                })
            }
            "fn" => {
                // Anonymous fn — stryke syntax for anonymous subroutines
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
                } else if self.peek().is_term_start()
                    && !(matches!(self.peek(), Token::Ident(ref kw) if kw == "sub")
                        && matches!(self.peek_at(1), Token::Ident(_)))
                    && !(self.suppress_parenless_call > 0 && matches!(self.peek(), Token::Ident(_)))
                    && !(matches!(self.peek(), Token::LBrace)
                        && self.peek_line() > self.prev_line())
                {
                    // Perl allows func arg without parens
                    // Guard: `sub <name> { }` is a named sub declaration (new
                    // statement), not an argument to the preceding call.
                    // Guard: suppress_parenless_call > 0 with Ident prevents consuming
                    // barewords (used by thread macro so `t Color::Red p` treats
                    // `p` as a stage, not an argument to the enum variant), but
                    // still allows `{` for struct/hash literals like `t Foo { x => 1 } p`.
                    // Guard: `{` on a new line is a new statement (hashref/block),
                    // not an argument to the preceding bareword call.
                    let args = self.parse_list_until_terminator()?;
                    Ok(Expr {
                        kind: ExprKind::FuncCall { name, args },
                        line,
                    })
                } else {
                    // No parens, no visible arguments — emit a Bareword.
                    // At runtime, Bareword tries sub resolution first (zero-arg
                    // call) and falls back to a string value.  stryke extension
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
        // Check for filehandle: print STDERR "msg"  /  print $fh "msg"
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
        } else if let Token::ScalarVar(ref v) = self.peek().clone() {
            // `print $fh "msg"` — scalar variable as indirect filehandle.
            // Treat as handle when the next token (after $var) is a term-start or
            // string literal *without* a preceding comma/operator, matching Perl's
            // indirect-object heuristic.
            // Exclude `$_` — it's virtually always the topic variable, not a handle.
            // Exclude `[` and `{` — those are array/hash subscripts on the variable
            // itself (`print $F[0]`, `print $h{k}`), not separate print arguments.
            // Exclude statement modifiers (`if`/`unless`/`while`/`until`/`for`/`foreach`)
            // — `print $_ if COND` prints `$_` to STDOUT, not to a handle named `$_`.
            let v = v.clone();
            if v == "_" {
                None
            } else {
                let saved = self.pos;
                self.advance();
                let next = self.peek().clone();
                let is_stmt_modifier = matches!(&next, Token::Ident(kw)
                    if matches!(kw.as_str(), "if" | "unless" | "while" | "until" | "for" | "foreach"));
                if !is_stmt_modifier
                    && !matches!(next, Token::LBracket | Token::LBrace)
                    && (next.is_term_start()
                        || matches!(
                            next,
                            Token::DoubleString(_)
                                | Token::BacktickString(_)
                                | Token::SingleString(_)
                        ))
                {
                    // Next token looks like a print argument — $var is the handle.
                    Some(format!("${v}"))
                } else {
                    self.pos = saved;
                    None
                }
            }
        } else {
            None
        };
        // `print()` / `say()` / `printf()` — empty parens default to `$_`,
        // matching Perl 5: `perldoc -f print` / `-f say` say "If no arguments
        // are given, prints $_." (Same convention as the topic-default unary
        // builtins handled in `parse_one_arg_or_default`.)
        let args =
            if matches!(self.peek(), Token::LParen) && matches!(self.peek_at(1), Token::RParen) {
                let line_topic = self.peek_line();
                self.advance(); // (
                self.advance(); // )
                vec![Expr {
                    kind: ExprKind::ScalarVar("_".into()),
                    line: line_topic,
                }]
            } else {
                self.parse_list_until_terminator()?
            };
        Ok(Expr {
            kind: make(handle, args),
            line,
        })
    }

    fn parse_block_list(&mut self) -> PerlResult<(Block, Expr)> {
        let block = self.parse_block()?;
        let block_end_line = self.prev_line();
        self.eat(&Token::Comma);
        // On the RHS of `|>`, the list operand is supplied by the piped LHS
        // and will be substituted at desugar time — accept a placeholder when
        // we're at a terminator here or on a new line (implicit semicolon).
        if self.in_pipe_rhs()
            && (matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            ) || self.peek_line() > block_end_line)
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
    fn parse_fan_count_and_block(&mut self, line: usize) -> PerlResult<(Option<Box<Expr>>, Block)> {
        // `fan { BLOCK }` — no count
        if matches!(self.peek(), Token::LBrace) {
            let block = self.parse_block()?;
            return Ok((None, block));
        }
        let saved = self.pos;
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
        } else if matches!(first.kind, ExprKind::Integer(_)) {
            // `fan COUNT EXPR` or `fan COUNT, EXPR` — integer count + body
            self.eat(&Token::Comma);
            let body = self.parse_fan_blockless_body(line)?;
            Ok((Some(Box::new(first)), body))
        } else {
            // Non-integer first (e.g. `$_`) followed by binary op (e.g. `* $_`)
            // — backtrack and re-parse as a full body expression.
            self.pos = saved;
            let body = self.parse_fan_blockless_body(line)?;
            Ok((None, body))
        }
    }

    /// Parse a blockless fan/fan_cap body as a full expression (not just postfix).
    fn parse_fan_blockless_body(&mut self, line: usize) -> PerlResult<Block> {
        if matches!(self.peek(), Token::LBrace) {
            return self.parse_block();
        }
        // Check for bareword (zero-arg sub call) terminated by ; } EOF , or pipe
        if let Token::Ident(ref name) = self.peek().clone() {
            if matches!(
                self.peek_at(1),
                Token::Comma | Token::Semicolon | Token::RBrace | Token::Eof | Token::PipeForward
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
        // Full expression (handles `$_ * $_`, `$_ + 1`, etc.)
        let expr = self.parse_assign_expr_stop_at_pipe()?;
        Ok(vec![Statement::new(StmtKind::Expression(expr), line)])
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
    /// True for any bareword the parser treats as a known builtin / keyword —
    /// Perl 5 core *or* a stryke extension. Used to suppress "call as user
    /// sub" interpretations (e.g. `sort my_cmp @list` only treats `my_cmp`
    /// as a comparator name if it *isn't* a known bareword). Previously named
    /// `is_perl_keyword`, which was misleading.
    fn is_known_bareword(name: &str) -> bool {
        Self::is_perl5_core(name) || Self::stryke_extension_name(name).is_some()
    }

    /// True iff `name` appears as any spelling (primary *or* alias) in a
    /// `try_builtin` match arm. Picks up the ~300 aliases that don't show
    /// up in the parser-level keyword lists but are still callable at
    /// runtime — so `map { tj }` can default to `tj($_)` the same way
    /// `map { to_json }` does.
    fn is_try_builtin_name(name: &str) -> bool {
        crate::builtins::BUILTIN_ARMS
            .iter()
            .any(|arm| arm.contains(&name))
    }

    /// True iff `name` is a Perl 5 core keyword/builtin (as shipped in stock
    /// `perl`). Extensions (`pmap`, `fan`, `timer`, …) are *not* included
    /// here — those live in `stryke_extension_name`. `%stryke::perl_compats`
    /// is derived from this list by `build.rs`.
    fn is_perl5_core(name: &str) -> bool {
        matches!(
            name,
            // ── array / list ────────────────────────────────────────────
            "map" | "grep" | "sort" | "reverse" | "join" | "split"
            | "push" | "pop" | "shift" | "unshift" | "splice"
            | "pack" | "unpack"
            // ── hash ────────────────────────────────────────────────────
            | "keys" | "values" | "each"
            // ── string ──────────────────────────────────────────────────
            | "chomp" | "chop" | "chr" | "ord" | "hex" | "oct"
            | "lc" | "uc" | "lcfirst" | "ucfirst"
            | "length" | "substr" | "index" | "rindex"
            | "sprintf" | "printf" | "print" | "say"
            | "pos" | "quotemeta" | "study"
            // ── numeric ─────────────────────────────────────────────────
            | "abs" | "int" | "sqrt" | "sin" | "cos" | "atan2"
            | "exp" | "log" | "rand" | "srand"
            // ── time ────────────────────────────────────────────────────
            | "time" | "localtime" | "gmtime"
            // ── type / reflection ───────────────────────────────────────
            | "defined" | "undef" | "ref" | "scalar" | "wantarray"
            | "caller" | "delete" | "exists" | "bless" | "prototype"
            | "tie" | "untie" | "tied"
            // ── io ──────────────────────────────────────────────────────
            | "open" | "close" | "read" | "readline" | "write" | "seek" | "tell"
            | "eof" | "binmode" | "getc" | "fileno" | "truncate"
            | "format" | "formline" | "select" | "vec"
            | "sysopen" | "sysread" | "sysseek" | "syswrite"
            // ── filesystem ──────────────────────────────────────────────
            | "stat" | "lstat" | "rename" | "unlink" | "utime"
            | "mkdir" | "rmdir" | "chdir" | "chmod" | "chown"
            | "glob" | "opendir" | "readdir" | "closedir"
            | "link" | "readlink" | "symlink"
            // ── ipc ─────────────────────────────────────────────────────
            | "fcntl" | "flock" | "ioctl" | "pipe" | "dbmopen" | "dbmclose"
            // ── sysv ipc ────────────────────────────────────────────────
            | "msgctl" | "msgget" | "msgrcv" | "msgsnd"
            | "semctl" | "semget" | "semop"
            | "shmctl" | "shmget" | "shmread" | "shmwrite"
            // ── process / system ────────────────────────────────────────
            | "system" | "exec" | "exit" | "die" | "warn" | "dump"
            | "fork" | "wait" | "waitpid" | "kill" | "alarm" | "sleep"
            | "chroot" | "times" | "umask" | "reset"
            | "getpgrp" | "setpgrp" | "getppid"
            | "getpriority" | "setpriority"
            // ── socket ──────────────────────────────────────────────────
            | "socket" | "socketpair" | "connect" | "listen" | "accept" | "shutdown"
            | "send" | "recv" | "bind" | "setsockopt" | "getsockopt"
            | "getpeername" | "getsockname"
            // ── posix metadata ──────────────────────────────────────────
            | "getpwnam" | "getpwuid" | "getpwent" | "setpwent"
            | "getgrnam" | "getgrgid" | "getgrent" | "setgrent"
            | "getlogin"
            | "gethostbyname" | "gethostbyaddr" | "gethostent"
            | "getnetbyname" | "getnetent"
            | "getprotobyname" | "getprotoent"
            | "getservbyname" | "getservent"
            | "sethostent" | "setnetent" | "setprotoent" | "setservent"
            | "endpwent" | "endgrent"
            | "endhostent" | "endnetent" | "endprotoent" | "endservent"
            // ── control flow ────────────────────────────────────────────
            | "return" | "do" | "eval" | "require"
            | "my" | "our" | "local" | "use" | "no"
            | "sub" | "if" | "unless" | "while" | "until"
            | "for" | "foreach" | "last" | "next" | "redo" | "goto"
            | "not" | "and" | "or"
            // ── quoting ─────────────────────────────────────────────────
            | "qw" | "qq" | "q"
            // ── phase blocks ────────────────────────────────────────────
            | "BEGIN" | "END"
        )
    }

    /// If `name` is a stryke-only extension keyword/builtin, return it; else `None`.
    /// Used by `--compat` to reject extensions at parse time.
    fn stryke_extension_name(name: &str) -> Option<&str> {
        match name {
            // ── parallel ────────────────────────────────────────────────────
            | "pmap" | "pmap_on" | "pflat_map" | "pflat_map_on" | "pmap_chunked"
            | "pgrep" | "pfor" | "psort" | "preduce" | "preduce_init" | "pmap_reduce"
            | "pcache" | "pchannel" | "pselect" | "puniq" | "pfirst" | "pany"
            | "fan" | "fan_cap" | "par_lines" | "par_walk" | "par_sed"
            | "par_find_files" | "par_line_count" | "pwatch" | "par_pipeline_stream"
            | "glob_par" | "ppool" | "barrier" | "pipeline" | "cluster"
            | "pmaps" | "pflat_maps" | "pgreps"
            // ── functional / iterator ───────────────────────────────────────
            | "fore" | "e" | "ep" | "flat_map" | "flat_maps" | "maps" | "filter" | "fi" | "find_all" | "reduce" | "fold"
            | "inject" | "collect" | "uniq" | "distinct" | "any" | "all" | "none"
            | "first" | "detect" | "find" | "compact" | "concat" | "chain" | "reject" | "flatten" | "set"
            | "min_by" | "max_by" | "sort_by" | "tally" | "find_index"
            | "each_with_index" | "count" | "cnt" |"len" | "group_by" | "chunk_by"
            | "zip" | "chunk" | "chunked" | "sliding_window" | "windowed"
            | "enumerate" | "with_index" | "shuffle" | "shuffled"| "heap"
            | "take_while" | "drop_while" | "skip_while" | "tap" | "peek" | "partition"
            | "zip_with" | "count_by" | "skip" | "first_or"
            // ── pipeline / string helpers ───────────────────────────────────
            | "input" | "lines" | "words" | "chars" | "digits" | "letters" | "letters_uc" | "letters_lc"
            | "punctuation" | "punct"
            | "sentences" | "sents"
            | "paragraphs" | "paras" | "sections" | "sects"
            | "numbers" | "nums" | "graphemes" | "grs" | "columns" | "cols"
            | "trim" | "avg" | "stddev"
            | "squared" | "sq" | "square" | "cubed" | "cb" | "cube" | "expt" | "pow" | "pw"
            | "normalize" | "snake_case" | "camel_case" | "kebab_case"
            | "frequencies" | "freq" | "interleave" | "ddump" | "stringify" | "str" | "top"
            | "to_json" | "to_csv" | "to_toml" | "to_yaml" | "to_xml"
            | "to_html" | "to_markdown" | "to_table" | "xopen"
            | "from_json" | "from_csv" | "from_toml" | "from_yaml" | "from_xml"
            | "clip" | "clipboard" | "paste" | "pbcopy" | "pbpaste" | "preview"
            | "sparkline" | "spark" | "bar_chart" | "bars" | "flame" | "flamechart"
            | "histo" | "gauge" | "spinner" | "spinner_start" | "spinner_stop"
            | "to_hash" | "to_set"
            | "to_file" | "read_lines" | "append_file" | "write_json" | "read_json"
            | "tempfile" | "tempdir" | "list_count" | "list_size" | "size"
            | "clamp" | "grep_v" | "select_keys" | "pluck" | "glob_match" | "which_all"
            | "dedup" | "nth" | "tail" | "take" | "drop" | "tee" | "range"
            | "inc" | "dec" | "elapsed"
            // ── filesystem extensions ───────────────────────────────────────
            | "files" | "filesf" | "f" | "fr" | "dirs" | "d" | "dr" | "sym_links"
            | "sockets" | "pipes" | "block_devices" | "char_devices"
            | "basename" | "dirname" | "fileparse" | "realpath" | "canonpath"
            | "copy" | "move" | "spurt" | "read_bytes" | "which"
            | "getcwd" | "touch" | "gethostname" | "uname"
            // ── data / network ──────────────────────────────────────────────
            | "csv_read" | "csv_write" | "dataframe" | "sqlite"
            | "fetch" | "fetch_json" | "fetch_async" | "fetch_async_json"
            | "par_fetch" | "par_csv_read" | "par_pipeline"
            | "json_encode" | "json_decode" | "json_jq"
            | "http_request" | "serve" | "ssh"
            | "html_parse" | "css_select" | "xml_parse" | "xpath"
            | "smtp_send"
            | "net_interfaces" | "net_ipv4" | "net_ipv6" | "net_mac"
            | "net_public_ip" | "net_dns" | "net_reverse_dns"
            | "net_ping" | "net_port_open" | "net_ports_scan"
            | "net_latency" | "net_download" | "net_headers"
            | "net_dns_servers" | "net_gateway" | "net_whois" | "net_hostname"
            // ── git ─────────────────────────────────────────────────────────
            | "git_log" | "git_status" | "git_diff" | "git_branches"
            | "git_tags" | "git_blame" | "git_authors" | "git_files"
            | "git_show" | "git_root"
            // ── audio / media ───────────────────────────────────────────────
            | "audio_convert" | "audio_info" | "id3_read" | "id3_write"
            // ── pdf ─────────────────────────────────────────────────────────
            | "to_pdf" | "pdf_text" | "pdf_pages"
            // ── serialization (stryke-only encoders) ────────────────────────
            | "toml_encode" | "toml_decode"
            | "yaml_encode" | "yaml_decode"
            | "xml_encode" | "xml_decode"
            // ── crypto / encoding ───────────────────────────────────────────
            | "md5" | "sha1" | "sha224" | "sha256" | "sha384" | "sha512"
            | "sha3_256" | "s3_256" | "sha3_512" | "s3_512"
            | "shake128" | "shake256"
            | "hmac_sha256" | "hmac_sha1" | "hmac_sha384" | "hmac_sha512" | "hmac_md5"
            | "uuid" | "crc32"
            | "blake2b" | "b2b" | "blake2s" | "b2s" | "blake3" | "b3"
            | "ripemd160" | "rmd160" | "md4"
            | "xxh32" | "xxhash32" | "xxh64" | "xxhash64" | "xxh3" | "xxhash3" | "xxh3_128" | "xxhash3_128"
            | "murmur3" | "murmur3_32" | "murmur3_128"
            | "siphash" | "siphash_keyed"
            | "hkdf_sha256" | "hkdf" | "hkdf_sha512"
            | "poly1305" | "poly1305_mac"
            | "base32_encode" | "b32e" | "base32_decode" | "b32d"
            | "base58_encode" | "b58e" | "base58_decode" | "b58d"
            | "totp" | "totp_generate" | "totp_verify" | "hotp" | "hotp_generate"
            | "aes_cbc_encrypt" | "aes_cbc_enc" | "aes_cbc_decrypt" | "aes_cbc_dec"
            | "blowfish_encrypt" | "bf_enc" | "blowfish_decrypt" | "bf_dec"
            | "des3_encrypt" | "3des_enc" | "tdes_enc" | "des3_decrypt" | "3des_dec" | "tdes_dec"
            | "twofish_encrypt" | "tf_enc" | "twofish_decrypt" | "tf_dec"
            | "camellia_encrypt" | "cam_enc" | "camellia_decrypt" | "cam_dec"
            | "cast5_encrypt" | "cast5_enc" | "cast5_decrypt" | "cast5_dec"
            | "salsa20" | "salsa20_encrypt" | "salsa20_decrypt"
            | "xsalsa20" | "xsalsa20_encrypt" | "xsalsa20_decrypt"
            | "secretbox" | "secretbox_seal" | "secretbox_open"
            | "nacl_box_keygen" | "box_keygen" | "nacl_box" | "nacl_box_seal" | "box_seal"
            | "nacl_box_open" | "box_open"
            | "qr_ascii" | "qr" | "qr_png" | "qr_svg"
            | "barcode_code128" | "code128" | "barcode_code39" | "code39"
            | "barcode_ean13" | "ean13" | "barcode_svg"
            | "argon2_hash" | "argon2" | "argon2_verify"
            | "bcrypt_hash" | "bcrypt" | "bcrypt_verify"
            | "scrypt_hash" | "scrypt" | "scrypt_verify"
            | "pbkdf2" | "pbkdf2_derive"
            | "random_bytes" | "randbytes" | "random_bytes_hex" | "randhex"
            | "aes_encrypt" | "aes_enc" | "aes_decrypt" | "aes_dec"
            | "chacha_encrypt" | "chacha_enc" | "chacha_decrypt" | "chacha_dec"
            | "rsa_keygen" | "rsa_encrypt" | "rsa_enc" | "rsa_decrypt" | "rsa_dec"
            | "rsa_encrypt_pkcs1" | "rsa_decrypt_pkcs1" | "rsa_sign" | "rsa_verify"
            | "ecdsa_p256_keygen" | "p256_keygen" | "ecdsa_p256_sign" | "p256_sign"
            | "ecdsa_p256_verify" | "p256_verify"
            | "ecdsa_p384_keygen" | "p384_keygen" | "ecdsa_p384_sign" | "p384_sign"
            | "ecdsa_p384_verify" | "p384_verify"
            | "ecdsa_secp256k1_keygen" | "secp256k1_keygen"
            | "ecdsa_secp256k1_sign" | "secp256k1_sign"
            | "ecdsa_secp256k1_verify" | "secp256k1_verify"
            | "ecdh_p256" | "p256_dh" | "ecdh_p384" | "p384_dh"
            | "ed25519_keygen" | "ed_keygen" | "ed25519_sign" | "ed_sign"
            | "ed25519_verify" | "ed_verify"
            | "x25519_keygen" | "x_keygen" | "x25519_dh" | "x_dh"
            | "base64_encode" | "base64_decode"
            | "hex_encode" | "hex_decode"
            | "url_encode" | "url_decode"
            | "gzip" | "gunzip" | "gz" | "ugz" | "zstd" | "zstd_decode" | "zst" | "uzst"
            | "brotli" | "br" | "brotli_decode" | "ubr"
            | "xz" | "lzma" | "xz_decode" | "unxz" | "unlzma"
            | "bzip2" | "bz2" | "bzip2_decode" | "bunzip2" | "ubz2"
            | "lz4" | "lz4_decode" | "unlz4"
            | "snappy" | "snp" | "snappy_decode" | "unsnappy"
            | "lzw" | "lzw_decode" | "unlzw"
            | "tar_create" | "tar" | "tar_extract" | "untar" | "tar_list"
            | "tar_gz_create" | "tgz" | "tar_gz_extract" | "untgz"
            | "zip_create" | "zip_archive" | "zip_extract" | "unzip_archive" | "zip_list"
            // ── special math functions ────────────────────────────────────────
            | "erf" | "erfc" | "gamma" | "tgamma" | "lgamma" | "ln_gamma"
            | "digamma" | "psi" | "beta_fn" | "lbeta" | "ln_beta"
            | "betainc" | "beta_reg" | "gammainc" | "gamma_li"
            | "gammaincc" | "gamma_ui" | "gammainc_reg" | "gamma_lr"
            | "gammaincc_reg" | "gamma_ur"
            // ── date / time ─────────────────────────────────────────────────
            | "datetime_utc" | "datetime_now_tz"
            | "datetime_format_tz" | "datetime_add_seconds"
            | "datetime_from_epoch"
            | "datetime_parse_rfc3339" | "datetime_parse_local"
            | "datetime_strftime"
            | "dateseq" | "dategrep" | "dateround" | "datesort"
            // ── jwt ─────────────────────────────────────────────────────────
            | "jwt_encode" | "jwt_decode" | "jwt_decode_unsafe"
            // ── logging ─────────────────────────────────────────────────────
            | "log_info" | "log_warn" | "log_error"
            | "log_debug" | "log_trace" | "log_json" | "log_level"
            // ── concurrency / timing ────────────────────────────────────────
            | "async" | "spawn" | "trace" | "timer" | "bench"
            | "eval_timeout" | "retry" | "rate_limit" | "every"
            | "gen" | "watch"
            // ── testing framework ────────────────────────────────────────────
            | "assert_eq" | "assert_ne" | "assert_ok" | "assert_err"
            | "assert_true" | "assert_false"
            | "assert_gt" | "assert_lt" | "assert_ge" | "assert_le"
            | "assert_match" | "assert_contains" | "assert_near" | "assert_dies"
            | "test_run"
            // ── system info ─────────────────────────────────────────────────
            | "mounts" | "du" | "du_tree" | "process_list"
            | "thread_count" | "pool_info" | "par_bench"
            // ── I/O extensions ──────────────────────────────────────────────
            | "slurp" | "cat" | "c" | "capture" | "pager" | "pg" | "less"
            | "stdin"
            // ── internal ────────────────────────────────────────────────────
            | "__stryke_rust_compile"
            // ── short aliases ───────────────────────────────────────────────
            | "p" | "rev"
            // ── trivial numeric / predicate builtins ────────────────────────
            | "even" | "odd" | "zero" | "nonzero"
            | "positive" | "pos_n" | "negative" | "neg_n"
            | "sign" | "negate" | "double" | "triple" | "half"
            | "identity" | "id"
            | "round" | "floor" | "ceil" | "ceiling" | "trunc" | "truncn"
            | "gcd" | "lcm" | "min2" | "max2"
            | "log2" | "log10" | "hypot"
            | "rad_to_deg" | "r2d" | "deg_to_rad" | "d2r"
            | "pow2" | "abs_diff"
            | "factorial" | "fact" | "fibonacci" | "fib"
            | "is_prime" | "is_square" | "is_power_of_two" | "is_pow2"
            | "cbrt" | "exp2" | "percent" | "pct" | "inverse"
            | "median" | "mode_val" | "variance"
            // ── trivial string ops ──────────────────────────────────────────
            | "is_empty" | "is_blank" | "is_numeric"
            | "is_upper" | "is_lower" | "is_alpha" | "is_digit" | "is_alnum"
            | "is_space" | "is_whitespace"
            | "starts_with" | "sw" | "ends_with" | "ew" | "contains"
            | "capitalize" | "cap" | "swap_case" | "repeat"
            | "title_case" | "title" | "squish"
            | "pad_left" | "lpad" | "pad_right" | "rpad" | "center"
            | "truncate_at" | "shorten" | "reverse_str" | "rev_str"
            | "char_count" | "word_count" | "wc" | "line_count" | "lc_lines"
            // ── trivial type predicates ─────────────────────────────────────
            | "is_array" | "is_arrayref" | "is_hash" | "is_hashref"
            | "is_code" | "is_coderef" | "is_ref"
            | "is_undef" | "is_defined" | "is_def"
            | "is_string" | "is_str" | "is_int" | "is_integer" | "is_float"
            // ── hash helpers ────────────────────────────────────────────────
            | "invert" | "merge_hash"
            | "has_key" | "hk" | "has_any_key" | "has_all_keys"
            // ── boolean combinators ─────────────────────────────────────────
            | "both" | "either" | "neither" | "xor_bool" | "bool_to_int" | "b2i"
            // ── collection helpers (trivial) ────────────────────────────────
            | "riffle" | "intersperse" | "every_nth"
            | "drop_n" | "take_n" | "rotate" | "swap_pairs"
            // ── base conversion ─────────────────────────────────────────────
            | "to_bin" | "bin_of" | "to_hex" | "hex_of" | "to_oct" | "oct_of"
            | "from_bin" | "from_hex" | "from_oct" | "to_base" | "from_base"
            | "bits_count" | "popcount" | "leading_zeros" | "lz"
            | "trailing_zeros" | "tz" | "bit_length" | "bitlen"
            // ── bit ops ─────────────────────────────────────────────────────
            | "bit_and" | "bit_or" | "bit_xor" | "bit_not"
            | "shift_left" | "shl" | "shift_right" | "shr"
            | "bit_set" | "bit_clear" | "bit_toggle" | "bit_test"
            // ── unit conversions: temperature ───────────────────────────────
            | "c_to_f" | "f_to_c" | "c_to_k" | "k_to_c" | "f_to_k" | "k_to_f"
            // ── unit conversions: distance ──────────────────────────────────
            | "miles_to_km" | "km_to_miles" | "miles_to_m" | "m_to_miles"
            | "feet_to_m" | "m_to_feet" | "inches_to_cm" | "cm_to_inches"
            | "yards_to_m" | "m_to_yards"
            // ── unit conversions: mass ──────────────────────────────────────
            | "kg_to_lbs" | "lbs_to_kg" | "g_to_oz" | "oz_to_g"
            | "stone_to_kg" | "kg_to_stone"
            // ── unit conversions: digital ───────────────────────────────────
            | "bytes_to_kb" | "b_to_kb" | "kb_to_bytes" | "kb_to_b"
            | "bytes_to_mb" | "mb_to_bytes" | "bytes_to_gb" | "gb_to_bytes"
            | "kb_to_mb" | "mb_to_gb"
            | "bits_to_bytes" | "bytes_to_bits"
            // ── unit conversions: time ──────────────────────────────────────
            | "seconds_to_minutes" | "s_to_m" | "minutes_to_seconds" | "m_to_s"
            | "seconds_to_hours" | "hours_to_seconds"
            | "seconds_to_days" | "days_to_seconds"
            | "minutes_to_hours" | "hours_to_minutes"
            | "hours_to_days" | "days_to_hours"
            // ── date helpers ────────────────────────────────────────────────
            | "is_leap_year" | "is_leap" | "days_in_month"
            | "month_name" | "month_short"
            | "weekday_name" | "weekday_short" | "quarter_of"
            // ── now / timestamp ─────────────────────────────────────────────
            | "now_ms" | "now_us" | "now_ns"
            | "unix_epoch" | "epoch" | "unix_epoch_ms" | "epoch_ms"
            // ── color / ANSI ────────────────────────────────────────────────
            | "rgb_to_hex" | "hex_to_rgb"
            | "ansi_red" | "ansi_green" | "ansi_yellow" | "ansi_blue"
            | "ansi_magenta" | "ansi_cyan" | "ansi_white" | "ansi_black"
            | "ansi_bold" | "ansi_dim" | "ansi_underline" | "ansi_reverse"
            | "strip_ansi"
            | "red" | "green" | "yellow" | "blue" | "magenta" | "purple" | "cyan"
            | "white" | "black" | "bold" | "dim" | "italic" | "underline"
            | "strikethrough" | "ansi_off" | "off" | "gray" | "grey"
            | "bright_red" | "bright_green" | "bright_yellow" | "bright_blue"
            | "bright_magenta" | "bright_cyan" | "bright_white"
            | "bg_red" | "bg_green" | "bg_yellow" | "bg_blue"
            | "bg_magenta" | "bg_cyan" | "bg_white" | "bg_black"
            | "red_bold" | "bold_red" | "green_bold" | "bold_green"
            | "yellow_bold" | "bold_yellow" | "blue_bold" | "bold_blue"
            | "magenta_bold" | "bold_magenta" | "cyan_bold" | "bold_cyan"
            | "white_bold" | "bold_white"
            | "blink" | "rapid_blink" | "hidden" | "overline"
            | "bg_bright_red" | "bg_bright_green" | "bg_bright_yellow" | "bg_bright_blue"
            | "bg_bright_magenta" | "bg_bright_cyan" | "bg_bright_white"
            | "rgb" | "bg_rgb" | "color256" | "c256" | "bg_color256" | "bg_c256"
            // ── network / validation ────────────────────────────────────────
            | "ipv4_to_int" | "int_to_ipv4"
            | "is_valid_ipv4" | "is_valid_ipv6" | "is_valid_email" | "is_valid_url"
            // ── path helpers ────────────────────────────────────────────────
            | "path_ext" | "path_stem" | "path_parent" | "path_join" | "path_split"
            | "strip_prefix" | "strip_suffix" | "ensure_prefix" | "ensure_suffix"
            // ── functional primitives ───────────────────────────────────────
            | "const_fn" | "always_true" | "always_false"
            | "flip_args" | "first_arg" | "second_arg" | "last_arg"
            // ── more list helpers ───────────────────────────────────────────
            | "count_eq" | "count_ne" | "all_eq"
            | "all_distinct" | "all_unique" | "has_duplicates"
            | "sum_of" | "product_of" | "max_of" | "min_of" | "range_of"
            // ── string quote / escape ───────────────────────────────────────
            | "quote" | "single_quote" | "unquote"
            | "extract_between" | "ellipsis"
            // ── random ──────────────────────────────────────────────────────
            | "coin_flip" | "dice_roll"
            | "random_int" | "random_float" | "random_bool"
            | "random_choice" | "random_between"
            | "random_string" | "random_alpha" | "random_digit"
            // ── system introspection ────────────────────────────────────────
            | "os_name" | "os_arch" | "num_cpus"
            | "pid" | "ppid" | "uid" | "gid"
            | "username" | "home_dir" | "temp_dir"
            | "mem_total" | "mem_free" | "mem_used"
            | "swap_total" | "swap_free" | "swap_used"
            | "disk_total" | "disk_free" | "disk_avail" | "disk_used"
            | "load_avg" | "sys_uptime" | "page_size"
            | "os_version" | "os_family" | "endianness" | "pointer_width"
            | "proc_mem" | "rss"
            // ── collection more ─────────────────────────────────────────────
            | "transpose" | "unzip"
            | "run_length_encode" | "rle" | "run_length_decode" | "rld"
            | "sliding_pairs" | "consecutive_eq" | "flatten_deep"
            // ── trig / math (batch 2) ───────────────────────────────────────
            | "tan" | "asin" | "acos" | "atan"
            | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh"
            | "sqr" | "cube_fn"
            | "mod_op" | "ceil_div" | "floor_div"
            | "is_finite" | "is_infinite" | "is_inf" | "is_nan"
            | "degrees" | "radians"
            | "min_abs" | "max_abs"
            | "saturate" | "sat01" | "wrap_around"
            // ── string (batch 2) ────────────────────────────────────────────
            | "rot13" | "rot47" | "caesar_shift" | "reverse_words"
            | "count_vowels" | "count_consonants" | "is_vowel" | "is_consonant"
            | "first_word" | "last_word"
            | "left_str" | "head_str" | "right_str" | "tail_str" | "mid_str"
            | "lowercase" | "uppercase"
            | "pascal_case" | "pc_case"
            | "constant_case" | "upper_snake" | "dot_case" | "path_case"
            | "is_palindrome" | "hamming_distance"
            | "longest_common_prefix" | "lcp"
            | "ascii_ord" | "ascii_chr" | "count_char" | "indexes_of"
            | "replace_first" | "replace_all_str"
            | "contains_any" | "contains_all"
            | "starts_with_any" | "ends_with_any"
            // ── predicates (batch 2) ────────────────────────────────────────
            | "is_pair" | "is_triple"
            | "is_sorted" | "is_asc" | "is_sorted_desc" | "is_desc"
            | "is_empty_arr" | "is_empty_hash"
            | "is_subset" | "is_superset" | "is_permutation"
            // ── collection (batch 2) ────────────────────────────────────────
            | "first_eq" | "last_eq"
            | "index_of" | "last_index_of" | "positions_of"
            | "batch" | "binary_search" | "bsearch" | "linear_search" | "lsearch"
            | "distinct_count" | "longest" | "shortest"
            | "array_union" | "list_union"
            | "array_intersection" | "list_intersection"
            | "array_difference" | "list_difference"
            | "symmetric_diff" | "group_of_n" | "chunk_n"
            | "repeat_list" | "cycle_n" | "random_sample" | "sample_n"
            // ── hash ops (batch 2) ──────────────────────────────────────────
            | "pick_keys" | "pick" | "omit_keys" | "omit"
            | "map_keys_fn" | "map_values_fn"
            | "hash_size" | "hash_from_pairs" | "pairs_from_hash"
            | "hash_eq" | "keys_sorted" | "values_sorted" | "remove_keys"
            // ── date (batch 2) ──────────────────────────────────────────────
            | "today" | "yesterday" | "tomorrow" | "is_weekend" | "is_weekday"
            // ── json helpers ────────────────────────────────────────────────
            | "json_pretty" | "json_minify" | "escape_json" | "json_escape"
            // ── process / env ───────────────────────────────────────────────
            | "cmd_exists" | "env_get" | "env_has" | "env_keys"
            | "argc" | "script_name"
            | "has_stdin_tty" | "has_stdout_tty" | "has_stderr_tty"
            // ── id helpers ──────────────────────────────────────────────────
            | "uuid_v4" | "nanoid" | "short_id" | "is_uuid" | "token"
            // ── url / email parts ───────────────────────────────────────────
            | "email_domain" | "email_local"
            | "url_host" | "url_path" | "url_query" | "url_scheme"
            // ── file stat / path ────────────────────────────────────────────
            | "file_size" | "fsize" | "file_mtime" | "mtime"
            | "file_atime" | "atime" | "file_ctime" | "ctime"
            | "is_symlink" | "is_readable" | "is_writable" | "is_executable"
            | "path_is_abs" | "path_is_rel"
            // ── stats / sort / array / format / cmp / regex / time conv / volume / force ──
            | "min_max" | "percentile" | "harmonic_mean" | "geometric_mean" | "zscore"
            | "sorted" | "sorted_desc" | "sorted_nums" | "sorted_by_length"
            | "reverse_list" | "list_reverse"
            | "without" | "without_nth" | "take_last" | "drop_last"
            | "pairwise" | "zipmap"
            | "format_bytes" | "human_bytes"
            | "format_duration" | "human_duration"
            | "format_number" | "group_number"
            | "format_percent" | "pad_number"
            | "spaceship" | "cmp_num" | "cmp_str"
            | "compare_versions" | "version_cmp"
            | "hash_insert" | "hash_update" | "hash_delete"
            | "matches_regex" | "re_match"
            | "count_regex_matches" | "regex_extract"
            | "regex_split_str" | "regex_replace_str"
            | "shuffle_chars" | "random_char" | "nth_word"
            | "head_lines" | "tail_lines" | "count_substring"
            | "is_valid_hex" | "hex_upper" | "hex_lower"
            | "ms_to_s" | "s_to_ms" | "ms_to_ns" | "ns_to_ms"
            | "us_to_ns" | "ns_to_us"
            | "liters_to_gallons" | "gallons_to_liters"
            | "liters_to_ml" | "ml_to_liters"
            | "cups_to_ml" | "ml_to_cups"
            | "newtons_to_lbf" | "lbf_to_newtons"
            | "joules_to_cal" | "cal_to_joules"
            | "watts_to_hp" | "hp_to_watts"
            | "pascals_to_psi" | "psi_to_pascals"
            | "bar_to_pascals" | "pascals_to_bar"
            // ── algebraic match ─────────────────────────────────────────────
            | "match"
            // ── clojure stdlib (only names not matched above) ─────────────────
            | "fst" | "rest" | "rst" | "second" | "snd"
            | "last_clj" | "lastc" | "butlast" | "bl"
            | "ffirst" | "ffs" | "fnext" | "fne" | "nfirst" | "nfs" | "nnext" | "nne"
            | "cons" | "conj"
            | "peek_clj" | "pkc" | "pop_clj" | "popc"
            | "some" | "not_any" | "not_every"
            | "comp" | "compose" | "partial" | "constantly" | "complement" | "compl"
            | "fnil" | "juxt"
            | "memoize" | "memo" | "curry" | "once"
            | "deep_clone" | "dclone" | "deep_merge" | "dmerge" | "deep_equal" | "deq"
            | "iterate" | "iter" | "repeatedly" | "rptd" | "cycle" | "cyc"
            | "mapcat" | "mcat" | "keep" | "kp" | "remove_clj" | "remc"
            | "reductions" | "rdcs"
            | "partition_by" | "pby" | "partition_all" | "pall"
            | "split_at" | "spat" | "split_with" | "spw"
            | "assoc" | "dissoc" | "get_in" | "gin" | "assoc_in" | "ain" | "update_in" | "uin"
            | "into" | "empty_clj" | "empc" | "seq" | "vec_clj" | "vecc"
            | "apply" | "appl"
            // ── python/ruby stdlib ───────────────────────────────────────────
            | "divmod" | "dm" | "accumulate" | "accum" | "starmap" | "smap"
            | "zip_longest" | "zipl" | "combinations" | "comb" | "permutations" | "perm"
            | "cartesian_product" | "cprod" | "compress" | "cmpr" | "filterfalse" | "falf"
            | "islice" | "isl" | "chain_from" | "chfr" | "pairwise_iter" | "pwi"
            | "tee_iter" | "teei" | "groupby_iter" | "gbi"
            | "each_slice" | "eslice" | "each_cons" | "econs"
            | "one" | "none_match" | "nonem"
            | "find_index_fn" | "fidx" | "rindex_fn" | "ridx"
            | "minmax" | "mmx" | "minmax_by" | "mmxb"
            | "dig" | "values_at" | "vat" | "fetch_val" | "fv" | "slice_arr" | "sla"
            | "transform_keys" | "tkeys" | "transform_values" | "tvals"
            | "sum_by" | "sumb" | "uniq_by" | "uqb"
            | "flat_map_fn" | "fmf" | "then_fn" | "thfn" | "times_fn" | "timf"
            | "step" | "upto" | "downto"
            // ── javascript array/object methods ─────────────────────────────
            | "find_last" | "fndl" | "find_last_index" | "fndli"
            | "at_index" | "ati" | "replace_at" | "repa"
            | "to_sorted" | "tsrt" | "to_reversed" | "trev" | "to_spliced" | "tspl"
            | "flat_depth" | "fltd" | "fill_arr" | "filla" | "includes_val" | "incv"
            | "object_keys" | "okeys" | "object_values" | "ovals"
            | "object_entries" | "oents" | "object_from_entries" | "ofents"
            // ── haskell list functions ──────────────────────────────────────
            | "span_fn" | "spanf" | "break_fn" | "brkf" | "group_runs" | "gruns"
            | "nub" | "sort_on" | "srton"
            | "intersperse_val" | "isp" | "intercalate" | "ical"
            | "replicate_val" | "repv" | "elem_of" | "elof" | "not_elem" | "ntelm"
            | "lookup_assoc" | "lkpa" | "scanl" | "scanr" | "unfoldr" | "unfr"
            // ── rust iterator methods ───────────────────────────────────────
            | "find_map" | "fndm" | "filter_map" | "fltm" | "fold_right" | "fldr"
            | "partition_either" | "peith" | "try_fold" | "tfld"
            | "map_while" | "mapw" | "inspect" | "insp"
            // ── ruby enumerable extras ──────────────────────────────────────
            | "tally_by" | "talb" | "sole" | "chunk_while" | "chkw" | "count_while" | "cntw"
            // ── go/general functional utilities ─────────────────────────────
            | "insert_at" | "insa" | "delete_at" | "dela" | "update_at" | "upda"
            | "split_on" | "spon" | "words_from" | "wfrm" | "unwords" | "unwds"
            | "lines_from" | "lfrm" | "unlines" | "unlns"
            | "window_n" | "winn" | "adjacent_pairs" | "adjp"
            | "zip_all" | "zall" | "unzip_pairs" | "uzp"
            | "interpose" | "ipos" | "partition_n" | "partn"
            | "map_indexed" | "mapi" | "reduce_indexed" | "redi" | "filter_indexed" | "flti"
            | "group_by_fn" | "gbf" | "index_by" | "idxb" | "associate" | "assoc_fn"
            // ── additional missing stdlib functions ─────────────────────────
            | "combinations_rep" | "combrep" | "inits" | "tails" | "subsequences" | "subseqs"
            | "nub_by" | "nubb" | "slice_when" | "slcw" | "slice_before" | "slcb" | "slice_after" | "slca"
            | "each_with_object" | "ewo" | "reduce_right" | "redr"
            | "is_sorted_by" | "issrtb" | "intersperse_with" | "ispw"
            | "running_reduce" | "runred" | "windowed_circular" | "wincirc"
            | "distinct_by" | "distb" | "average" | "mean" | "copy_within" | "cpyw"
            | "and_list" | "andl" | "or_list" | "orl" | "concat_map" | "cmap"
            | "elem_index" | "elidx" | "elem_indices" | "elidxs" | "find_indices" | "fndidxs"
            | "delete_first" | "delfst" | "delete_by" | "delby" | "insert_sorted" | "inssrt"
            | "union_list" | "unionl" | "intersect_list" | "intl"
            | "maximum_by" | "maxby" | "minimum_by" | "minby" | "batched" | "btch"
            // ── Extended stdlib: Text Processing ─────────────────────────────
            | "match_all" | "mall" | "capture_groups" | "capg" | "is_match" | "ism"
            | "split_regex" | "splre" | "replace_regex" | "replre"
            | "is_ascii" | "isasc" | "to_ascii" | "toasc"
            | "char_at" | "chat" | "code_point_at" | "cpat" | "from_code_point" | "fcp"
            | "normalize_spaces" | "nrmsp" | "remove_whitespace" | "rmws"
            | "pluralize" | "plur" | "ordinalize" | "ordn"
            | "parse_int" | "pint" | "parse_float" | "pflt" | "parse_bool" | "pbool"
            | "levenshtein" | "lev" | "soundex" | "sdx" | "similarity" | "sim"
            | "common_prefix" | "cpfx" | "common_suffix" | "csfx"
            | "wrap_text" | "wrpt" | "dedent" | "ddt" | "indent" | "idt"
            // ── Extended stdlib: Advanced Numeric ────────────────────────────
            | "lerp" | "inv_lerp" | "ilerp" | "smoothstep" | "smst" | "remap"
            | "dot_product" | "dotp" | "cross_product" | "crossp"
            | "matrix_mul" | "matmul" | "mm"
            | "magnitude" | "mag" | "normalize_vec" | "nrmv"
            | "distance" | "dist" | "manhattan_distance" | "mdist"
            | "covariance" | "cov" | "correlation" | "corr"
            | "iqr" | "quantile" | "qntl" | "clamp_int" | "clpi"
            | "in_range" | "inrng" | "wrap_range" | "wrprng"
            | "sum_squares" | "sumsq" | "rms" | "cumsum" | "csum" | "cumprod" | "cprod_acc" | "diff"
            // ── Extended stdlib: Date/Time ───────────────────────────────────
            | "add_days" | "addd" | "add_hours" | "addh" | "add_minutes" | "addm"
            | "diff_days" | "diffd" | "diff_hours" | "diffh"
            | "start_of_day" | "sod" | "end_of_day" | "eod"
            | "start_of_hour" | "soh" | "start_of_minute" | "som"
            // ── Extended stdlib: Encoding/Hashing ────────────────────────────
            | "urle" | "urld"
            | "html_encode" | "htmle" | "html_decode" | "htmld"
            | "adler32" | "adl32" | "fnv1a" | "djb2"
            // ── Extended stdlib: Validation ──────────────────────────────────
            | "is_credit_card" | "iscc" | "is_isbn10" | "isbn10" | "is_isbn13" | "isbn13"
            | "is_iban" | "isiban" | "is_hex_str" | "ishex" | "is_binary_str" | "isbin"
            | "is_octal_str" | "isoct" | "is_json" | "isjson" | "is_base64" | "isb64"
            | "is_semver" | "issv" | "is_slug" | "isslug" | "slugify" | "slug"
            // ── Extended stdlib: Collection Advanced ─────────────────────────
            | "mode_stat" | "mstat" | "sampn" | "weighted_sample" | "wsamp"
            | "shuffle_arr" | "shuf" | "argmax" | "amax" | "argmin" | "amin"
            | "argsort" | "asrt" | "rank" | "rnk" | "dense_rank" | "drnk"
            | "partition_point" | "ppt" | "lower_bound" | "lbound"
            | "upper_bound" | "ubound" | "equal_range" | "eqrng"
            // ── Extended stdlib: Matrix Operations ───────────────────────────
            | "matrix_add" | "madd" | "matrix_sub" | "msub" | "matrix_mult" | "mmult"
            | "matrix_scalar" | "mscal" | "matrix_identity" | "mident"
            | "matrix_zeros" | "mzeros" | "matrix_ones" | "mones"
            | "matrix_diag" | "mdiag" | "matrix_trace" | "mtrace"
            | "matrix_row" | "mrow" | "matrix_col" | "mcol"
            | "matrix_shape" | "mshape" | "matrix_det" | "mdet"
            | "matrix_scale" | "mat_scale" | "diagonal" | "diag"
            // ── Extended stdlib: Graph Algorithms ────────────────────────────
            | "topological_sort" | "toposort" | "bfs_traverse" | "bfs"
            | "dfs_traverse" | "dfs" | "shortest_path_bfs" | "spbfs"
            | "connected_components_graph" | "ccgraph"
            | "has_cycle_graph" | "hascyc" | "is_bipartite_graph" | "isbip"
            // ── Extended stdlib: Data Validation ─────────────────────────────
            | "is_ipv4_addr" | "isip4" | "is_ipv6_addr" | "isip6"
            | "is_mac_addr" | "ismac" | "is_port_num" | "isport"
            | "is_hostname_valid" | "ishost"
            | "is_iso_date" | "isisodt" | "is_iso_time" | "isisotm"
            | "is_iso_datetime" | "isisodtm"
            | "is_phone_num" | "isphone" | "is_us_zip" | "iszip"
            // ── Extended stdlib: String Utilities Novel ──────────────────────
            | "word_wrap_text" | "wwrap" | "center_text" | "ctxt"
            | "ljust_text" | "ljt" | "rjust_text" | "rjt" | "zfill_num" | "zfill"
            | "remove_all_str" | "rmall" | "replace_n_times" | "repln"
            | "find_all_indices" | "fndalli"
            | "text_between" | "txbtwn" | "text_before" | "txbef" | "text_after" | "txaft"
            | "text_before_last" | "txbefl" | "text_after_last" | "txaftl"
            // ── Extended stdlib: Math Novel ──────────────────────────────────
            | "is_even_num" | "iseven" | "is_odd_num" | "isodd"
            | "is_positive_num" | "ispos" | "is_negative_num" | "isneg"
            | "is_zero_num" | "iszero" | "is_whole_num" | "iswhole"
            | "log_with_base" | "logb" | "nth_root_of" | "nroot"
            | "frac_part" | "fracp" | "reciprocal_of" | "recip"
            | "copy_sign" | "cpsgn" | "fused_mul_add" | "fmadd"
            | "floor_mod" | "fmod" | "floor_div_op" | "fdivop"
            | "signum_of" | "sgnum" | "midpoint_of" | "midpt"
            // ── Extended stdlib batch 3: Array Analysis ──────────────────────
            | "longest_run" | "lrun" | "longest_increasing" | "linc"
            | "longest_decreasing" | "ldec" | "max_sum_subarray" | "maxsub"
            | "majority_element" | "majority" | "kth_largest" | "kthl"
            | "kth_smallest" | "kths" | "count_inversions" | "cinv"
            | "is_monotonic" | "ismono" | "equilibrium_index" | "eqidx"
            // ── Extended stdlib batch 3: Set Operations ──────────────────────
            | "jaccard_index" | "jaccard" | "dice_coefficient" | "dicecoef"
            | "overlap_coefficient" | "overlapcoef"
            | "power_set" | "powerset" | "cartesian_power" | "cartpow"
            // ── Extended stdlib batch 3: Advanced String ─────────────────────
            | "is_isogram" | "isiso" | "is_heterogram" | "ishet"
            | "hamdist" | "jaro_similarity" | "jarosim"
            | "longest_common_substring" | "lcsub"
            | "longest_common_subsequence" | "lcseq"
            | "count_words" | "wcount" | "count_lines" | "lcount"
            | "count_chars" | "ccount" | "count_bytes" | "bcount"
            // ── Extended stdlib batch 3: More Math ───────────────────────────
            | "binomial" | "binom" | "catalan" | "catn" | "pascal_row" | "pascrow"
            | "is_coprime" | "iscopr" | "euler_totient" | "etot"
            | "mobius" | "mob" | "is_squarefree" | "issqfr"
            | "digital_root" | "digroot" | "is_narcissistic" | "isnarc"
            | "is_harshad" | "isharsh" | "is_kaprekar" | "iskap"
            // ── Extended stdlib batch 3: Date/Time Additional ────────────────
            | "day_of_year" | "doy" | "week_of_year" | "woy"
            | "days_in_month_fn" | "daysinmo" | "is_valid_date" | "isvdate"
            | "age_in_years" | "ageyrs"
            // ── functional combinators ──────────────────────────────────────

            | "when_true" | "when_false" | "if_else" | "clamp_fn"
            | "attempt" | "try_fn" | "safe_div" | "safe_mod" | "safe_sqrt" | "safe_log"
            | "juxt2" | "juxt3" | "tap_val" | "debug_val" | "converge"
            | "iterate_n" | "unfold" | "arity_of" | "is_callable"
            | "coalesce" | "default_to" | "fallback"
            | "apply_list" | "zip_apply" | "scan"
            | "keep_if" | "reject_if" | "group_consecutive"
            | "after_n" | "before_n" | "clamp_list" | "normalize_list" | "softmax"

            // ── matrix / linear algebra ─────────────────────────────────────


            | "matrix_multiply" | "mat_mul"
            | "identity_matrix" | "eye" | "zeros_matrix" | "zeros" | "ones_matrix" | "ones"



            | "vec_normalize" | "unit_vec" | "vec_add" | "vec_sub" | "vec_scale"
            | "linspace" | "arange"
            // ── more regex ──────────────────────────────────────────────────
            | "re_test" | "re_find_all" | "re_groups" | "re_escape"
            | "re_split_limit" | "glob_to_regex" | "is_regex_valid"
            // ── more process / system ───────────────────────────────────────
            | "cwd" | "pwd_str" | "cpu_count" | "is_root" | "uptime_secs"
            | "env_pairs" | "env_set" | "env_remove" | "hostname_str" | "is_tty" | "signal_name"
            // ── data structure helpers ───────────────────────────────────────
            | "stack_new" | "queue_new" | "lru_new"
            | "counter" | "counter_most_common" | "defaultdict" | "ordered_set"
            | "bitset_new" | "bitset_set" | "bitset_test" | "bitset_clear"
            // ── trivial numeric helpers (batch 4) ─────────────────────────────
            | "abs_ceil" | "abs_each" | "abs_floor" | "ceil_each" | "dec_each"
            | "double_each" | "floor_each" | "half_each" | "inc_each" | "length_each"
            | "negate_each" | "not_each" | "offset_each" | "reverse_each" | "round_each"
            | "scale_each" | "sqrt_each" | "square_each" | "to_float_each" | "to_int_each"
            | "trim_each" | "type_each" | "upcase_each" | "downcase_each" | "bool_each"
            // ── math / physics constants ──────────────────────────────────────
            | "avogadro" | "boltzmann" | "golden_ratio" | "gravity" | "ln10" | "ln2"
            | "planck" | "speed_of_light" | "sqrt2"
            // ── physics formulas ──────────────────────────────────────────────
            | "bmi_calc" | "compound_interest" | "dew_point" | "discount_amount"
            | "force_mass_acc" | "freq_wavelength" | "future_value" | "haversine"
            | "heat_index" | "kinetic_energy" | "margin_price" | "markup_price"
            | "mortgage_payment" | "ohms_law_i" | "ohms_law_r" | "ohms_law_v"
            | "potential_energy" | "present_value" | "simple_interest" | "speed_distance_time"
            | "tax_amount" | "tip_amount" | "wavelength_freq" | "wind_chill"
            // ── math functions ────────────────────────────────────────────────
            | "angle_between_deg" | "approx_eq" | "chebyshev_distance" | "copysign"
            | "cosine_similarity" | "cube_root" | "entropy" | "float_bits" | "fma"
            | "int_bits" | "jaccard_similarity" | "log_base" | "mae" | "mse" | "nth_root"
            | "r_squared" | "reciprocal" | "relu" | "rmse" | "rotate_point" | "round_to"
            | "sigmoid" | "signum" | "square_root"
            // ── sequences ─────────────────────────────────────────────────────
            | "cubes_seq" | "fibonacci_seq" | "powers_of_seq" | "primes_seq"
            | "squares_seq" | "triangular_seq"
            // ── string helpers (batch 4) ──────────────────────────────────────
            | "alternate_case" | "angle_bracket" | "bracket" | "byte_length"
            | "bytes_to_hex_str" | "camel_words" | "char_length" | "chars_to_string"
            | "chomp_str" | "chop_str" | "filter_chars" | "from_csv_line" | "hex_to_bytes"
            | "insert_str" | "intersperse_char" | "ljust" | "map_chars" | "mirror_string"
            | "normalize_whitespace" | "only_alnum" | "only_alpha" | "only_ascii"
            | "only_digits" | "parenthesize" | "remove_str" | "repeat_string" | "rjust"
            | "sentence_case" | "string_count" | "string_sort" | "string_to_chars"
            | "string_unique_chars" | "substring" | "to_csv_line" | "trim_left" | "trim_right"
            | "xor_strings"
            // ── list helpers (batch 4) ─────────────────────────────────────────
            | "adjacent_difference" | "append_elem" | "consecutive_pairs" | "contains_elem"
            | "count_elem" | "drop_every" | "duplicate_count" | "elem_at" | "find_first"
            | "first_elem" | "flatten_once" | "fold_left" | "from_digits" | "from_pairs"
            | "group_by_size" | "hash_filter_keys" | "hash_from_list" | "hash_map_values"
            | "hash_merge_deep" | "hash_to_list" | "hash_zip" | "head_n" | "histogram_bins"
            | "index_of_elem" | "init_list" | "interleave_lists" | "last_elem" | "least_common"
            | "list_compact" | "list_eq" | "list_flatten_deep" | "max_list" | "mean_list"
            | "min_list" | "mode_list" | "most_common" | "partition_two" | "prefix_sums"
            | "prepend" | "product_list" | "remove_at" | "remove_elem" | "remove_first_elem"
            | "repeat_elem" | "running_max" | "running_min" | "sample_one" | "scan_left"
            | "second_elem" | "span" | "suffix_sums" | "sum_list" | "tail_n" | "take_every"
            | "third_elem" | "to_array" | "to_pairs" | "trimmed_mean" | "unique_count_of"
            | "wrap_index" | "digits_of"
            // ── predicates (batch 4) ──────────────────────────────────────────
            | "all_match" | "any_match" | "is_between" | "is_blank_or_nil" | "is_divisible_by"
            | "is_email" | "is_even" | "is_falsy" | "is_fibonacci" | "is_hex_color"
            | "is_in_range" | "is_ipv4" | "is_multiple_of" | "is_negative" | "is_nil"
            | "is_nonzero" | "is_odd" | "is_perfect_square" | "is_positive" | "is_power_of"
            | "is_prefix" | "is_present" | "is_strictly_decreasing" | "is_strictly_increasing"
            | "is_suffix" | "is_triangular" | "is_truthy" | "is_url" | "is_whole" | "is_zero"
            // ── counters (batch 4) ────────────────────────────────────────────
            | "count_digits" | "count_letters" | "count_lower" | "count_match"
            | "count_punctuation" | "count_spaces" | "count_upper" | "defined_count"
            | "empty_count" | "falsy_count" | "nonempty_count" | "numeric_count"
            | "truthy_count" | "undef_count"
            // ── conversion / utility (batch 4) ────────────────────────────────
            | "assert_type" | "between" | "clamp_each" | "die_if" | "die_unless"
            | "join_colons" | "join_commas" | "join_dashes" | "join_dots" | "join_lines"
            | "join_pipes" | "join_slashes" | "join_spaces" | "join_tabs" | "measure"
            | "max_float" | "min_float" | "noop_val" | "nop" | "pass" | "pred" | "succ"
            | "tap_debug" | "to_bool" | "to_float" | "to_int" | "to_string" | "void"
            | "range_exclusive" | "range_inclusive"
            // ── math / numeric (uncategorized batch) ────────────────────────────
            | "aliquot_sum" | "autocorrelation" | "bell_number" | "cagr" | "coeff_of_variation"
            | "collatz_length" | "collatz_sequence" | "convolution" | "cross_entropy"
            | "depreciation_double" | "depreciation_linear" | "discount" | "divisors"
            | "epsilon" | "euclidean_distance" | "euler_number" | "exponential_moving_average"
            | "f64_max" | "f64_min" | "fft_magnitude" | "goldbach" | "i64_max" | "i64_min"
            | "kurtosis" | "linear_regression" | "look_and_say" | "lucas" | "luhn_check"
            | "mean_absolute_error" | "mean_squared_error" | "median_absolute_deviation"
            | "minkowski_distance" | "moving_average" | "multinomial" | "neg_inf" | "npv"
            | "num_divisors" | "partition_number" | "pascals_triangle" | "skewness"
            | "standard_error" | "subfactorial" | "sum_divisors" | "totient_sum"
            | "tribonacci" | "weighted_mean" | "winsorize"
            // ── statistics (extended) ─────────────────────────────────────────
            | "chi_square_stat" | "describe" | "five_number_summary"
            | "gini" | "gini_coefficient" | "lorenz_curve" | "outliers_iqr"
            | "percentile_rank" | "quartiles" | "sample_stddev" | "sample_variance"
            | "spearman_correlation" | "t_test_one_sample" | "t_test_two_sample"
            | "z_score" | "z_scores"
            // ── number theory / primes ──────────────────────────────────────────
            | "abundant_numbers" | "deficient_numbers" | "is_abundant" | "is_deficient"
            | "is_pentagonal" | "is_perfect" | "is_smith" | "next_prime" | "nth_prime"
            | "pentagonal_number" | "perfect_numbers" | "prev_prime" | "prime_factors"
            | "prime_pi" | "primes_up_to" | "triangular_number" | "twin_primes"
            // ── geometry / physics ──────────────────────────────────────────────
            | "area_circle" | "area_ellipse" | "area_rectangle" | "area_trapezoid" | "area_triangle"
            | "bearing" | "circumference" | "cone_volume" | "cylinder_volume" | "heron_area"
            | "midpoint" | "perimeter_rectangle" | "perimeter_triangle" | "point_distance"
            | "polygon_area" | "slope" | "sphere_surface" | "sphere_volume" | "triangle_hypotenuse"
            // ── geometry (extended) ───────────────────────────────────────────
            | "angle_between" | "arc_length" | "bounding_box" | "centroid"
            | "circle_from_three_points" | "convex_hull" | "ellipse_perimeter"
            | "frustum_volume" | "haversine_distance" | "line_intersection"
            | "point_in_polygon" | "polygon_perimeter" | "pyramid_volume"
            | "reflect_point" | "scale_point" | "sector_area"
            | "torus_surface" | "torus_volume" | "translate_point"
            | "vector_angle" | "vector_cross" | "vector_dot" | "vector_magnitude" | "vector_normalize"
            // ── constants ───────────────────────────────────────────────────────
            | "avogadro_number" | "boltzmann_constant" | "electron_mass" | "elementary_charge"
            | "gravitational_constant" | "phi" | "pi" | "planck_constant" | "proton_mass"
            | "sol" | "tau"
            // ── finance ─────────────────────────────────────────────────────────
            | "bac_estimate" | "bmi" | "break_even" | "margin" | "markup" | "roi" | "tax" | "tip"
            // ── finance (extended) ────────────────────────────────────────────
            | "amortization_schedule" | "black_scholes_call" | "black_scholes_put"
            | "bond_price" | "bond_yield" | "capm" | "continuous_compound"
            | "discounted_payback" | "duration" | "irr"
            | "max_drawdown" | "modified_duration" | "nper" | "num_periods" | "payback_period"
            | "pmt" | "pv" | "rule_of_72" | "sharpe_ratio" | "sortino_ratio"
            | "wacc" | "xirr"
            // ── string processing (uncategorized batch) ─────────────────────────
            | "acronym" | "atbash" | "bigrams" | "camel_to_snake" | "char_frequencies"
            | "chunk_string" | "collapse_whitespace" | "dedent_text" | "indent_text"
            | "initials" | "leetspeak" | "mask_string" | "ngrams" | "pig_latin"
            | "remove_consonants" | "remove_vowels" | "reverse_each_word" | "snake_to_camel"
            | "sort_words" | "string_distance" | "string_multiply" | "strip_html"
            | "trigrams" | "unique_words" | "word_frequencies" | "zalgo"
            // ── encoding / phonetics ────────────────────────────────────────────
            | "braille_encode" | "double_metaphone" | "metaphone" | "morse_decode"
            | "morse_encode" | "nato_phonetic" | "phonetic_digit" | "subscript" | "superscript"
            | "to_emoji_num"
            // ── roman numerals ──────────────────────────────────────────────────
            | "int_to_roman" | "roman_add" | "roman_numeral_list" | "roman_to_int"
            // ── base / gray code ────────────────────────────────────────────────
            | "base_convert" | "binary_to_gray" | "gray_code_sequence" | "gray_to_binary"
            // ── color operations ────────────────────────────────────────────────
            | "ansi_256" | "ansi_truecolor" | "color_blend" | "color_complement"
            | "color_darken" | "color_distance" | "color_grayscale" | "color_invert"
            | "color_lighten" | "hsl_to_rgb" | "hsv_to_rgb" | "random_color"
            | "rgb_to_hsl" | "rgb_to_hsv"
            // ── matrix operations (uncategorized batch) ─────────────────────────
            | "matrix_flatten" | "matrix_from_rows" | "matrix_hadamard" | "matrix_inverse"
            | "matrix_map" | "matrix_max" | "matrix_min" | "matrix_power" | "matrix_sum"
            | "matrix_transpose"
            // ── array / list operations (uncategorized batch) ───────────────────
            | "binary_insert" | "bucket" | "clamp_array" | "group_consecutive_by"
            | "histogram" | "merge_sorted" | "next_permutation" | "normalize_array"
            | "normalize_range" | "peak_detect" | "range_compress" | "range_expand"
            | "reservoir_sample" | "run_length_decode_str" | "run_length_encode_str"
            | "zero_crossings"
            // ── DSP / signal (extended) ───────────────────────────────────────
            | "apply_window" | "bandpass_filter" | "cross_correlation" | "dft"
            | "downsample" | "energy" | "envelope" | "highpass_filter" | "idft"
            | "lowpass_filter" | "median_filter" | "normalize_signal" | "phase_spectrum"
            | "power_spectrum" | "resample" | "spectral_centroid" | "spectrogram" | "upsample"
            | "window_blackman" | "window_hamming" | "window_hann" | "window_kaiser"
            // ── validation predicates (uncategorized batch) ─────────────────────
            | "is_anagram" | "is_balanced_parens" | "is_control" | "is_numeric_string"
            | "is_pangram" | "is_printable" | "is_valid_cidr" | "is_valid_cron"
            | "is_valid_hex_color" | "is_valid_latitude" | "is_valid_longitude" | "is_valid_mime"
            // ── algorithms / puzzles ────────────────────────────────────────────
            | "eval_rpn" | "fizzbuzz" | "game_of_life_step" | "mandelbrot_char"
            | "sierpinski" | "tower_of_hanoi" | "truth_table"
            // ── misc / utility ──────────────────────────────────────────────────
            | "byte_size" | "degrees_to_compass" | "to_string_val" | "type_of"
            // ── math formulas ───────────────────────────────────────────────────
            | "quadratic_roots" | "quadratic_discriminant" | "arithmetic_series"
            | "geometric_series" | "stirling_approx"
            | "double_factorial" | "rising_factorial" | "falling_factorial"
            | "gamma_approx" | "erf_approx" | "normal_pdf" | "normal_cdf"
            | "poisson_pmf" | "exponential_pdf" | "inverse_lerp"
            | "map_range"
            // ── physics formulas ────────────────────────────────────────────────
            | "momentum" | "impulse" | "work" | "power_phys" | "torque" | "angular_velocity"
            | "centripetal_force" | "escape_velocity" | "orbital_velocity" | "orbital_period"
            | "gravitational_force" | "coulomb_force" | "electric_field" | "capacitance"
            | "capacitor_energy" | "inductor_energy" | "resonant_frequency"
            | "rc_time_constant" | "rl_time_constant" | "impedance_rlc"
            | "relativistic_mass" | "lorentz_factor" | "time_dilation" | "length_contraction"
            | "relativistic_energy" | "rest_energy" | "de_broglie_wavelength"
            | "photon_energy" | "photon_energy_wavelength" | "schwarzschild_radius"
            | "stefan_boltzmann" | "wien_displacement" | "ideal_gas_pressure" | "ideal_gas_volume"
            | "projectile_range" | "projectile_max_height" | "projectile_time"
            | "spring_force" | "spring_energy" | "pendulum_period" | "doppler_frequency"
            | "decibel_ratio" | "snells_law" | "brewster_angle" | "critical_angle"
            | "lens_power" | "thin_lens" | "magnification_lens"
            // ── math constants ──────────────────────────────────────────────────
            | "euler_mascheroni" | "apery_constant" | "feigenbaum_delta" | "feigenbaum_alpha"
            | "catalan_constant" | "khinchin_constant" | "glaisher_constant"
            | "plastic_number" | "silver_ratio" | "supergolden_ratio"
            // ── physics constants ───────────────────────────────────────────────
            | "vacuum_permittivity" | "vacuum_permeability" | "coulomb_constant"
            | "fine_structure_constant" | "rydberg_constant" | "bohr_radius"
            | "bohr_magneton" | "nuclear_magneton" | "stefan_boltzmann_constant"
            | "wien_constant" | "gas_constant" | "faraday_constant" | "neutron_mass"
            | "atomic_mass_unit" | "earth_mass" | "earth_radius" | "sun_mass" | "sun_radius"
            | "astronomical_unit" | "light_year" | "parsec" | "hubble_constant"
            | "planck_length" | "planck_time" | "planck_mass" | "planck_temperature"
            // ── linear algebra (extended) ──────────────────────────────────
            | "matrix_solve" | "msolve" | "solve"
            | "matrix_lu" | "mlu" | "matrix_qr" | "mqr"
            | "matrix_eigenvalues" | "meig" | "eigenvalues" | "eig"
            | "matrix_norm" | "mnorm" | "matrix_cond" | "mcond" | "cond"
            | "matrix_pinv" | "mpinv" | "pinv"
            | "matrix_cholesky" | "mchol" | "cholesky"
            | "matrix_det_general" | "mdetg" | "det"
            // ── statistics tests (extended) ────────────────────────────────
            | "welch_ttest" | "welcht" | "paired_ttest" | "pairedt"
            | "cohen_d" | "cohend" | "anova_oneway" | "anova" | "anova1"
            | "spearman_corr" | "rho" | "kendall_tau" | "kendall" | "ktau"
            | "confidence_interval" | "ci"
            // ── distributions (extended) ──────────────────────────────────
            | "beta_pdf" | "betapdf" | "gamma_pdf" | "gammapdf"
            | "chi2_pdf" | "chi2pdf" | "chi_squared_pdf"
            | "t_pdf" | "tpdf" | "student_pdf"
            | "f_pdf" | "fpdf" | "fisher_pdf"
            | "lognormal_pdf" | "lnormpdf" | "weibull_pdf" | "weibpdf"
            | "cauchy_pdf" | "cauchypdf" | "laplace_pdf" | "laplacepdf"
            | "pareto_pdf" | "paretopdf"
            // ── interpolation & curve fitting ─────────────────────────────
            | "lagrange_interp" | "lagrange" | "linterp"
            | "cubic_spline" | "cspline" | "spline"
            | "poly_eval" | "polyval" | "polynomial_fit" | "polyfit"
            // ── numerical integration & differentiation ───────────────────
            | "trapz" | "trapezoid" | "simpson" | "simps"
            | "numerical_diff" | "numdiff" | "diff_array"
            | "cumtrapz" | "cumulative_trapz"
            // ── optimization / root finding ────────────────────────────────
            | "bisection" | "bisect" | "newton_method" | "newton" | "newton_raphson"
            | "golden_section" | "golden" | "gss"
            // ── ODE solvers ───────────────────────────────────────────────
            | "rk4" | "runge_kutta" | "rk4_ode" | "euler_ode" | "euler_method"
            // ── graph algorithms (extended) ────────────────────────────────
            | "dijkstra" | "shortest_path" | "bellman_ford" | "bellmanford"
            | "floyd_warshall" | "floydwarshall" | "apsp"
            | "prim_mst" | "mst" | "prim"
            // ── trig extensions ───────────────────────────────────────────
            | "cot" | "sec" | "csc" | "acot" | "asec" | "acsc" | "sinc" | "versin" | "versine"
            // ── ML activation functions ───────────────────────────────────
            | "leaky_relu" | "lrelu" | "elu" | "selu" | "gelu"
            | "silu" | "swish" | "mish" | "softplus"
            | "hard_sigmoid" | "hardsigmoid" | "hard_swish" | "hardswish"
            // ── special functions ─────────────────────────────────────────
            | "bessel_j0" | "j0" | "bessel_j1" | "j1"
            | "lambert_w" | "lambertw" | "productlog"
            // ── number theory (extended) ──────────────────────────────────
            | "mod_exp" | "modexp" | "powmod"
            | "mod_inv" | "modinv" | "chinese_remainder" | "crt"
            | "miller_rabin" | "millerrabin" | "is_probable_prime"
            // ── combinatorics (extended) ──────────────────────────────────
            | "derangements" | "stirling2" | "stirling_second"
            | "bernoulli_number" | "bernoulli" | "harmonic_number" | "harmonic"
            // ── physics (new) ─────────────────────────────────────────────
            | "drag_force" | "fdrag" | "ideal_gas" | "pv_nrt"
            // ── financial greeks & risk ───────────────────────────────────
            | "bs_delta" | "bsdelta" | "option_delta"
            | "bs_gamma" | "bsgamma" | "option_gamma"
            | "bs_vega" | "bsvega" | "option_vega"
            | "bs_theta" | "bstheta" | "option_theta"
            | "bs_rho" | "bsrho" | "option_rho"
            | "bond_duration" | "mac_duration"
            // ── DSP extensions ────────────────────────────────────────────
            | "dct" | "idct" | "goertzel" | "chirp" | "chirp_signal"
            // ── encoding extensions ───────────────────────────────────────
            | "base85_encode" | "b85e" | "ascii85_encode" | "a85e"
            | "base85_decode" | "b85d" | "ascii85_decode" | "a85d"
            // ── R base: distributions ─────────────────────────────────────
            | "pnorm" | "qnorm" | "pbinom" | "dbinom" | "ppois"
            | "punif" | "pexp" | "pweibull" | "plnorm" | "pcauchy"
            // ── R base: matrix ops ────────────────────────────────────────
            | "rbind" | "cbind"
            | "row_sums" | "rowSums" | "col_sums" | "colSums"
            | "row_means" | "rowMeans" | "col_means" | "colMeans"
            | "outer_product" | "outer" | "crossprod" | "tcrossprod"
            | "nrow" | "ncol" | "prop_table" | "proptable"
            // ── R base: vector ops ────────────────────────────────────────
            | "cummax" | "cummin" | "scale_vec" | "scale"
            | "which_fn" | "tabulate"
            | "duplicated" | "duped" | "rev_vec"
            | "seq_fn" | "rep_fn" | "rep"
            | "cut_bins" | "cut" | "find_interval" | "findInterval"
            | "ecdf_fn" | "ecdf" | "density_est" | "density"
            | "embed_ts" | "embed"
            // ── R base: stats tests ───────────────────────────────────────
            | "shapiro_test" | "shapiro" | "ks_test" | "ks"
            | "wilcox_test" | "wilcox" | "mann_whitney"
            | "prop_test" | "proptest" | "binom_test" | "binomtest"
            // ── R base: apply / functional ────────────────────────────────
            | "sapply" | "tapply" | "do_call" | "docall"
            // ── R base: ML / clustering ───────────────────────────────────
            | "kmeans" | "prcomp" | "pca"
            // ── R base: random generators ─────────────────────────────────
            | "rnorm" | "runif" | "rexp" | "rbinom" | "rpois" | "rgeom"
            | "rgamma" | "rbeta" | "rchisq" | "rt" | "rf"
            | "rweibull" | "rlnorm" | "rcauchy"
            // ── R base: quantile functions ────────────────────────────────
            | "qunif" | "qexp" | "qweibull" | "qlnorm" | "qcauchy"
            // ── R base: additional CDFs ───────────────────────────────────
            | "pgamma" | "pbeta" | "pchisq" | "pt_cdf" | "pt" | "pf_cdf" | "pf"
            // ── R base: additional PMFs ───────────────────────────────────
            | "dgeom" | "dunif" | "dnbinom" | "dhyper"
            // ── R base: smoothing / interpolation ─────────────────────────
            | "lowess" | "loess" | "approx_fn" | "approx"
            // ── R base: linear models ─────────────────────────────────────
            | "lm_fit" | "lm"
            // ── R base: remaining quantiles ───────────────────────────────
            | "qgamma" | "qbeta" | "qchisq" | "qt_fn" | "qt" | "qf_fn" | "qf"
            | "qbinom" | "qpois"
            // ── R base: time series ───────────────────────────────────────
            | "acf_fn" | "acf" | "pacf_fn" | "pacf"
            | "diff_lag" | "diff_ts" | "ts_filter" | "filter_ts"
            // ── R base: regression diagnostics ────────────────────────────
            | "predict_lm" | "predict" | "confint_lm" | "confint"
            // ── R base: multivariate stats ────────────────────────────────
            | "cor_matrix" | "cor_mat" | "cov_matrix" | "cov_mat"
            | "mahalanobis" | "mahal" | "dist_matrix" | "dist_mat"
            | "hclust" | "cutree" | "weighted_var" | "wvar" | "cov2cor"
            // ── SVG plotting ──────────────────────────────────────────────
            | "scatter_svg" | "scatter_plot" | "line_svg" | "line_plot"
            | "plot_svg" | "hist_svg" | "histogram_svg"
            | "boxplot_svg" | "box_plot" | "bar_svg" | "barchart_svg"
            | "pie_svg" | "pie_chart" | "heatmap_svg" | "heatmap"
            | "donut_svg" | "donut" | "area_svg" | "area_chart"
            | "hbar_svg" | "hbar" | "radar_svg" | "radar" | "spider"
            | "candlestick_svg" | "candlestick" | "ohlc"
            | "violin_svg" | "violin" | "cor_heatmap" | "cor_matrix_svg"
            | "stacked_bar_svg" | "stacked_bar"
            | "wordcloud_svg" | "wordcloud" | "wcloud"
            | "treemap_svg" | "treemap"
            | "pvw"
            // ── Cyberpunk terminal art ────────────────────────────────
            | "cyber_city" | "cyber_grid" | "cyber_rain" | "matrix_rain"
            | "cyber_glitch" | "glitch_text" | "cyber_banner" | "neon_banner"
            | "cyber_circuit" | "cyber_skull" | "cyber_eye"
            => Some(name),
            _ => None,
        }
    }

    /// Reserved hash names that cannot be shadowed by user declarations.
    /// These are stryke's reflection hashes populated from builtins metadata.
    fn is_reserved_hash_name(name: &str) -> bool {
        matches!(
            name,
            "b" | "pc"
                | "e"
                | "a"
                | "d"
                | "c"
                | "p"
                | "all"
                | "stryke::builtins"
                | "stryke::perl_compats"
                | "stryke::extensions"
                | "stryke::aliases"
                | "stryke::descriptions"
                | "stryke::categories"
                | "stryke::primaries"
                | "stryke::all"
        )
    }

    /// Check if a UDF name shadows a stryke builtin and error if so.
    /// Called only in non-compat mode — compat mode allows shadowing for Perl 5 parity.
    fn check_udf_shadows_builtin(&self, name: &str, line: usize) -> PerlResult<()> {
        if Self::is_known_bareword(name) || Self::is_try_builtin_name(name) {
            return Err(self.syntax_err(
                format!(
                    "cannot define sub `{name}`: shadows stryke builtin (use --compat for Perl 5 mode)"
                ),
                line,
            ));
        }
        Ok(())
    }

    /// Check if a hash name shadows a reserved stryke hash and error if so.
    /// Called only in non-compat mode.
    fn check_hash_shadows_reserved(&self, name: &str, line: usize) -> PerlResult<()> {
        if Self::is_reserved_hash_name(name) {
            return Err(self.syntax_err(
                format!(
                    "cannot declare hash `%{name}`: shadows stryke reserved hash (use --compat for Perl 5 mode)"
                ),
                line,
            ));
        }
        Ok(())
    }

    /// Validate assignment to %hash in non-compat mode.
    /// Rejects: scalar, string, arrayref, hashref, coderef, undef, odd-length list.
    fn validate_hash_assignment(&self, value: &Expr, line: usize) -> PerlResult<()> {
        match &value.kind {
            ExprKind::Integer(_) | ExprKind::Float(_) => {
                return Err(self.syntax_err(
                    "cannot assign scalar to hash — use %h = (key => value) or %h = %{$hashref}",
                    line,
                ));
            }
            ExprKind::String(_) | ExprKind::InterpolatedString(_) | ExprKind::Bareword(_) => {
                return Err(self.syntax_err(
                    "cannot assign string to hash — use %h = (key => value) or %h = %{$hashref}",
                    line,
                ));
            }
            ExprKind::ArrayRef(_) => {
                return Err(self.syntax_err(
                    "cannot assign arrayref to hash — use %h = @{$arrayref} for even-length list",
                    line,
                ));
            }
            ExprKind::ScalarRef(inner) => {
                if matches!(inner.kind, ExprKind::ArrayVar(_)) {
                    return Err(self.syntax_err(
                        "cannot assign \\@array to hash — use %h = @array for even-length list",
                        line,
                    ));
                }
                if matches!(inner.kind, ExprKind::HashVar(_)) {
                    return Err(self.syntax_err(
                        "cannot assign \\%hash to hash — use %h = %other directly",
                        line,
                    ));
                }
            }
            ExprKind::HashRef(_) => {
                return Err(self.syntax_err(
                    "cannot assign hashref to hash — use %h = %{$hashref} to dereference",
                    line,
                ));
            }
            ExprKind::CodeRef { .. } => {
                return Err(self.syntax_err("cannot assign coderef to hash", line));
            }
            ExprKind::Undef => {
                return Err(
                    self.syntax_err("cannot assign undef to hash — use %h = () to empty", line)
                );
            }
            ExprKind::List(items) if items.len() % 2 != 0 => {
                if !items.iter().any(|e| {
                    matches!(
                        e.kind,
                        ExprKind::ArrayVar(_)
                            | ExprKind::HashVar(_)
                            | ExprKind::FuncCall { .. }
                            | ExprKind::Deref { .. }
                            | ExprKind::ScalarVar(_)
                    )
                }) {
                    return Err(self.syntax_err(
                        format!(
                            "odd-length list ({} elements) in hash assignment — missing value for last key",
                            items.len()
                        ),
                        line,
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Validate assignment to @array in non-compat mode.
    /// Rejects: undef (likely a mistake — use `@a = ()` to empty).
    /// Note: bare scalars like `@a = 2` are allowed since Perl coerces them to single-element lists.
    /// Note: `@a = {hashref}` is allowed as a common pattern for single-element arrays.
    fn validate_array_assignment(&self, value: &Expr, line: usize) -> PerlResult<()> {
        if let ExprKind::Undef = &value.kind {
            return Err(
                self.syntax_err("cannot assign undef to array — use @a = () to empty", line)
            );
        }
        Ok(())
    }

    /// Validate assignment to $scalar in non-compat mode.
    /// Rejects: list literals (Perl 5 silently returns last element — footgun).
    fn validate_scalar_assignment(&self, value: &Expr, line: usize) -> PerlResult<()> {
        if let ExprKind::List(items) = &value.kind {
            if items.len() > 1 {
                return Err(self.syntax_err(
                    format!(
                        "cannot assign {}-element list to scalar — Perl 5 silently takes last element; use ($x) = (list) or $x = $list[-1]",
                        items.len()
                    ),
                    line,
                ));
            }
        }
        Ok(())
    }

    /// Validate an assignment based on target type (in non-compat mode only).
    fn validate_assignment(&self, target: &Expr, value: &Expr, line: usize) -> PerlResult<()> {
        if crate::compat_mode() {
            return Ok(());
        }
        match &target.kind {
            ExprKind::HashVar(_) => self.validate_hash_assignment(value, line),
            ExprKind::ArrayVar(_) => self.validate_array_assignment(value, line),
            ExprKind::ScalarVar(_) => self.validate_scalar_assignment(value, line),
            _ => Ok(()),
        }
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
        // Default to `$_` when the next token cannot start an argument expression
        // because it has lower precedence than a named unary operator. Perl 5
        // named unary precedence sits above ternary / comparison / logical / bitwise
        // / assignment / list ops; everything below should terminate the implicit
        // argument and let the surrounding expression continue.
        // See `perldoc perlop` ("Named Unary Operators").
        if matches!(
            self.peek(),
            // Statement / list / call boundaries
            Token::Semicolon
                | Token::RBrace
                | Token::RParen
                | Token::RBracket
                | Token::Eof
                | Token::Comma
                | Token::FatArrow
                | Token::PipeForward
            // Ternary `? :`
                | Token::Question
                | Token::Colon
            // Comparison / equality (numeric + string)
                | Token::NumEq | Token::NumNe | Token::NumLt | Token::NumGt
                | Token::NumLe | Token::NumGe | Token::Spaceship
                | Token::StrEq | Token::StrNe | Token::StrLt | Token::StrGt
                | Token::StrLe | Token::StrGe | Token::StrCmp
            // Logical (symbolic and word forms) + defined-or
                | Token::LogAnd | Token::LogOr | Token::LogNot
                | Token::LogAndWord | Token::LogOrWord | Token::LogNotWord
                | Token::DefinedOr
            // Range (lower precedence than named unary)
                | Token::Range | Token::RangeExclusive
            // Assignment (any compound form)
                | Token::Assign | Token::PlusAssign | Token::MinusAssign
                | Token::MulAssign | Token::DivAssign | Token::ModAssign
                | Token::PowAssign | Token::DotAssign | Token::AndAssign
                | Token::OrAssign | Token::XorAssign | Token::DefinedOrAssign
                | Token::ShiftLeftAssign | Token::ShiftRightAssign
                | Token::BitAndAssign | Token::BitOrAssign
        ) {
            return Ok(Expr {
                kind: ExprKind::ScalarVar("_".into()),
                line: self.peek_line(),
            });
        }
        // `f()` — empty parens default to `$_`, matching Perl 5 semantics.
        // `perldoc -f length`: "If EXPR is omitted, returns the length of $_."
        // Perl accepts both `length` and `length()` as `length($_)`.
        if matches!(self.peek(), Token::LParen) && matches!(self.peek_at(1), Token::RParen) {
            let line = self.peek_line();
            self.advance(); // (
            self.advance(); // )
            return Ok(Expr {
                kind: ExprKind::ScalarVar("_".into()),
                line,
            });
        }
        self.parse_one_arg()
    }

    /// Array operand for `shift` / `pop`: default `@_`, or `shift(@a)` / `shift()` (empty parens = `@_`).
    fn parse_one_arg_or_argv(&mut self) -> PerlResult<Expr> {
        let line = self.prev_line(); // line where shift/pop keyword was
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
        // Implicit semicolon: if next token is on a different line, don't consume it
        if matches!(
            self.peek(),
            Token::Semicolon
                | Token::RBrace
                | Token::RParen
                | Token::Eof
                | Token::Comma
                | Token::PipeForward
        ) || self.peek_line() > line
        {
            Ok(Expr {
                kind: ExprKind::ArrayVar("_".into()),
                line,
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
        } else if self.suppress_parenless_call > 0 && matches!(self.peek(), Token::Ident(_)) {
            // In thread context, don't consume barewords as arguments
            // so `t filesf sorted ep` parses `sorted` as a stage, not an arg to filesf
            Ok(vec![])
        } else {
            self.parse_list_until_terminator()
        }
    }

    /// Check if the next token is `=>` (fat arrow). If so, the preceding bareword
    /// should be treated as an auto-quoted string (hash key), not a function call.
    /// Returns `Some(Expr::String(name))` if fat arrow follows, `None` otherwise.
    #[inline]
    fn fat_arrow_autoquote(&self, name: &str, line: usize) -> Option<Expr> {
        if matches!(self.peek(), Token::FatArrow) {
            Some(Expr {
                kind: ExprKind::String(name.to_string()),
                line,
            })
        } else {
            None
        }
    }

    /// Parse a hash subscript key inside `{…}`.
    ///
    /// Perl auto-quotes a single bareword before `}`, even for keywords:
    /// `$h{print}`, `$r->{f}` etc. all yield the string key.
    fn parse_hash_subscript_key(&mut self) -> PerlResult<Expr> {
        let line = self.peek_line();
        if let Token::Ident(ref k) = self.peek().clone() {
            if matches!(self.peek_at(1), Token::RBrace) {
                let s = k.clone();
                self.advance();
                return Ok(Expr {
                    kind: ExprKind::String(s),
                    line,
                });
            }
        }
        self.parse_expression()
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
        let call_line = self.prev_line();
        loop {
            // `$g->next { ... }` — `{` starts the enclosing statement's block, not an anonymous
            // hash argument to `next` (paren-less method call has no args here).
            if args.is_empty() && matches!(self.peek(), Token::LBrace) {
                break;
            }
            if matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
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
            // Implicit semicolon: if no args collected yet and next token is on a different
            // line, treat newline as statement boundary. Allows `$p->method\nnext_stmt`.
            if args.is_empty() && self.peek_line() > call_line {
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
                // Assignment operators: `$obj->field = val` is setter sugar, not method arg.
                | Token::Assign
                | Token::PlusAssign
                | Token::MinusAssign
                | Token::MulAssign
                | Token::DivAssign
                | Token::ModAssign
                | Token::PowAssign
                | Token::DotAssign
                | Token::AndAssign
                | Token::OrAssign
                | Token::XorAssign
                | Token::DefinedOrAssign
                | Token::ShiftLeftAssign
                | Token::ShiftRightAssign
                | Token::BitAndAssign
                | Token::BitOrAssign
        )
    }

    fn parse_list_until_terminator(&mut self) -> PerlResult<Vec<Expr>> {
        let mut args = Vec::new();
        // Line of the last consumed token (the keyword / function name that
        // triggered this arg parse).  Used for implicit-semicolon: if no args
        // have been parsed yet and the next token is on a *different* line,
        // treat the newline as a statement boundary and stop.
        let call_line = self.prev_line();
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
            // Implicit semicolons: if no args have been collected yet and the
            // next token is on a different line from the call keyword, treat
            // the newline as a statement boundary.  This prevents paren-less
            // calls (`say`, `print`, user subs) from greedily swallowing the
            // *next* statement when the author omitted a semicolon.
            // After a comma continuation, multi-line arg lists still work.
            if args.is_empty() && self.peek_line() > call_line {
                break;
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
            // If the key expression is a hash/array variable and is followed by `}` or `,`
            // with no `=>`, treat the whole thing as a hash-from-expression construction.
            // This handles `{ %a }`, `{ %a, key => val }`, etc.
            if matches!(self.peek(), Token::RBrace | Token::Comma)
                && matches!(
                    key.kind,
                    ExprKind::HashVar(_)
                        | ExprKind::Deref {
                            kind: Sigil::Hash,
                            ..
                        }
                )
            {
                // Synthesize a pair whose key/value is spread from the hash expression.
                // Use a sentinel "spread" pair: key=the hash expr, value=undef.
                // The evaluator will flatten this.
                let sentinel_key = Expr {
                    kind: ExprKind::String("__HASH_SPREAD__".into()),
                    line,
                };
                pairs.push((sentinel_key, key));
                self.eat(&Token::Comma);
                continue;
            }
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

    /// Parse `key => val, key => val, ...` up to (but not consuming) `term`.
    /// Used by the `%[…]` and `%{k=>v,…}` sugar to build an inline hashref
    /// AST node, sidestepping the block/hashref ambiguity that `try_parse_hash_ref`
    /// navigates. Caller expects and consumes `term` itself.
    fn parse_hashref_pairs_until(&mut self, term: &Token) -> PerlResult<Vec<(Expr, Expr)>> {
        let mut pairs = Vec::new();
        while !matches!(&self.peek(), t if std::mem::discriminant(*t) == std::mem::discriminant(term))
            && !matches!(self.peek(), Token::Eof)
        {
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
            if self.eat(&Token::FatArrow) || self.eat(&Token::Comma) {
                let val = self.parse_assign_expr()?;
                pairs.push((key, val));
                self.eat(&Token::Comma);
            } else {
                return Err(self.syntax_err("Expected => or , in hash ref", key.line));
            }
        }
        Ok(pairs)
    }

    /// Inside an interpolated string, after a `$name`/`${EXPR}`/`$name[i]`/`$name{k}` base
    /// expression, consume any chain of `->[…]`, `->{…}`, **adjacent** `[…]`, or `{…}`
    /// subscripts. Perl auto-implies `->` between consecutive subscripts, so
    /// `$matrix[1][1]` is `$matrix[1]->[1]` and `$h{a}{b}` is `$h{a}->{b}`.
    /// Each step wraps the current expression in an `ArrowDeref`.
    fn interp_chain_subscripts(
        &self,
        chars: &[char],
        i: &mut usize,
        mut base: Expr,
        line: usize,
    ) -> Expr {
        loop {
            // Optional `->` connector
            let (after, requires_subscript) =
                if *i + 1 < chars.len() && chars[*i] == '-' && chars[*i + 1] == '>' {
                    (*i + 2, true)
                } else {
                    (*i, false)
                };
            if after >= chars.len() {
                break;
            }
            match chars[after] {
                '[' => {
                    *i = after + 1;
                    let mut idx_str = String::new();
                    while *i < chars.len() && chars[*i] != ']' {
                        idx_str.push(chars[*i]);
                        *i += 1;
                    }
                    if *i < chars.len() {
                        *i += 1;
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
                    base = Expr {
                        kind: ExprKind::ArrowDeref {
                            expr: Box::new(base),
                            index: Box::new(idx_expr),
                            kind: DerefKind::Array,
                        },
                        line,
                    };
                }
                '{' => {
                    *i = after + 1;
                    let mut key = String::new();
                    let mut depth = 1usize;
                    while *i < chars.len() && depth > 0 {
                        if chars[*i] == '{' {
                            depth += 1;
                        } else if chars[*i] == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        key.push(chars[*i]);
                        *i += 1;
                    }
                    if *i < chars.len() {
                        *i += 1;
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
                    base = Expr {
                        kind: ExprKind::ArrowDeref {
                            expr: Box::new(base),
                            index: Box::new(key_expr),
                            kind: DerefKind::Hash,
                        },
                        line,
                    };
                }
                _ => {
                    if requires_subscript {
                        // `->method()` etc — not interpolated, leave for literal output.
                    }
                    break;
                }
            }
        }
        base
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
                    // `${…}` — braced variable OR expression interpolation.
                    //   `${name}`              → ScalarVar(name)        (Perl standard)
                    //   `${$ref}` / `${\EXPR}` → deref the expression   (Perl standard)
                    //   `${name}[idx]` / `${name}{k}` / `${$r}[i]` …    chain after `}`
                    // stryke's prior `#{expr}` form remains supported elsewhere.
                    i += 1;
                    let mut inner = String::new();
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
                        inner.push(chars[i]);
                        i += 1;
                    }
                    if i < chars.len() {
                        i += 1; // skip closing }
                    }

                    // Distinguish "name" from "expression". If trimmed inner starts with
                    // `$`, `\`, or contains operator/punctuation chars, treat as Perl
                    // expression and emit a scalar deref. Otherwise, plain variable name.
                    let trimmed = inner.trim();
                    let is_expr = trimmed.starts_with('$')
                        || trimmed.starts_with('\\')
                        || trimmed.starts_with('@')   // `${@arr}` rare but valid
                        || trimmed.starts_with('%')   // `${%h}`   rare but valid
                        || trimmed.contains(['(', '+', '-', '*', '/', '.', '?', '&', '|']);
                    let mut base: Expr = if is_expr {
                        // Re-parse the inner content as a Perl expression. Wrap in
                        // `Deref { kind: Sigil::Scalar }` to dereference the resulting
                        // scalar reference (Perl: `${$r}` ≡ `$$r`).
                        match parse_expression_from_str(trimmed, "<interp>") {
                            Ok(e) => Expr {
                                kind: ExprKind::Deref {
                                    expr: Box::new(e),
                                    kind: Sigil::Scalar,
                                },
                                line,
                            },
                            Err(_) => Expr {
                                kind: ExprKind::ScalarVar(inner.clone()),
                                line,
                            },
                        }
                    } else {
                        // Treat as a plain (possibly qualified) variable name.
                        Expr {
                            kind: ExprKind::ScalarVar(inner),
                            line,
                        }
                    };

                    // After `${…}` we may see `[idx]` / `{key}` for indexing into the
                    // dereferenced array/hash (`${$ar}[1]`, `${$hr}{k}`), and arrow
                    // chains thereafter.
                    base = self.interp_chain_subscripts(&chars, &mut i, base, line);
                    parts.push(StringPart::Expr(base));
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
                    // `$_<`, `$_<<`, … — outer topic (stryke extension); only for bare `_`.
                    if name == "_" {
                        while i < chars.len() && chars[i] == '<' {
                            name.push('<');
                            i += 1;
                        }
                    }
                    // Build the base expression, then thread arrow-deref chains
                    // (`->[…]` / `->{…}`) onto it so things like `$ar->[2]`,
                    // `$href->{k}`, and chained `$x->{a}[1]->{b}` interpolate
                    // correctly inside double-quoted strings (Perl convention).
                    let mut base = if i < chars.len() && chars[i] == '{' {
                        // $hash{key}
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
                        Expr {
                            kind: ExprKind::HashElement {
                                hash: name,
                                key: Box::new(key_expr),
                            },
                            line,
                        }
                    } else if i < chars.len() && chars[i] == '[' {
                        // $array[idx]
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
                        Expr {
                            kind: ExprKind::ArrayElement {
                                array: name,
                                index: Box::new(idx_expr),
                            },
                            line,
                        }
                    } else {
                        // Bare $name — defer to the chain-extension loop below.
                        Expr {
                            kind: ExprKind::ScalarVar(name),
                            line,
                        }
                    };

                    // Chain `->[…]` / `->{…}` AND adjacent `[…]` / `{…}` — Perl
                    // implies `->` between consecutive subscripts (`$m[1][2]`
                    // ≡ `$m[1]->[2]`).  See `interp_chain_subscripts`.
                    base = self.interp_chain_subscripts(&chars, &mut i, base, line);
                    parts.push(StringPart::Expr(base));
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
                        i += 1;
                        // Check for hash element access: `$+{key}`, `$-{key}`, etc.
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
                            } // skip }
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
                            let mut base = Expr {
                                kind: ExprKind::HashElement {
                                    hash: probe,
                                    key: Box::new(key_expr),
                                },
                                line,
                            };
                            base = self.interp_chain_subscripts(&chars, &mut i, base, line);
                            parts.push(StringPart::Expr(base));
                        } else {
                            // Check for arrow deref chain: `$@->{key}`, etc.
                            let mut base = Expr {
                                kind: ExprKind::ScalarVar(probe),
                                line,
                            };
                            base = self.interp_chain_subscripts(&chars, &mut i, base, line);
                            if matches!(base.kind, ExprKind::ScalarVar(_)) {
                                // No chain extension — use the simpler ScalarVar part
                                if let ExprKind::ScalarVar(name) = base.kind {
                                    parts.push(StringPart::ScalarVar(name));
                                }
                            } else {
                                parts.push(StringPart::Expr(base));
                            }
                        }
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
            } else if chars[i] == '#'
                && i + 1 < chars.len()
                && chars[i + 1] == '{'
                && !crate::compat_mode()
            {
                // #{expr} — Ruby-style expression interpolation (stryke extension).
                if !literal.is_empty() {
                    parts.push(StringPart::Literal(std::mem::take(&mut literal)));
                }
                i += 2; // skip `#{`
                let mut inner = String::new();
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
                    inner.push(chars[i]);
                    i += 1;
                }
                if i < chars.len() {
                    i += 1; // skip closing `}`
                }
                let expr = parse_block_from_str(inner.trim(), "-e", line)?;
                parts.push(StringPart::Expr(expr));
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

/// Parse a statement list from `s` and wrap as `do { ... }` (for `#{...}` interpolation).
pub fn parse_block_from_str(s: &str, file: &str, line: usize) -> PerlResult<Expr> {
    let mut lexer = Lexer::new_with_file(s, file);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new_with_file(tokens, file);
    let stmts = parser.parse_statements()?;
    let inner_line = stmts.first().map(|st| st.line).unwrap_or(line);
    let inner = Expr {
        kind: ExprKind::CodeRef {
            params: vec![],
            body: stmts,
        },
        line: inner_line,
    };
    Ok(Expr {
        kind: ExprKind::Do(Box::new(inner)),
        line,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(code: &str) -> Program {
        let mut lexer = Lexer::new(code);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens);
        parser.parse_program().expect("parse")
    }

    fn parse_err(code: &str) -> String {
        let mut lexer = Lexer::new(code);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => return e.message,
        };
        let mut parser = Parser::new(tokens);
        parser.parse_program().unwrap_err().message
    }

    #[test]
    fn parse_empty_program() {
        let p = parse_ok("");
        assert!(p.statements.is_empty());
    }

    #[test]
    fn parse_semicolons_only() {
        let p = parse_ok(";;");
        assert!(p.statements.len() <= 3);
    }

    #[test]
    fn parse_simple_scalar_assignment() {
        let p = parse_ok("$x = 1");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_simple_array_assignment() {
        let p = parse_ok("@arr = (1, 2, 3)");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_simple_hash_assignment() {
        let p = parse_ok("%h = (a => 1, b => 2)");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_subroutine_decl() {
        let p = parse_ok("sub foo { 1 }");
        assert_eq!(p.statements.len(), 1);
        match &p.statements[0].kind {
            StmtKind::SubDecl { name, .. } => assert_eq!(name, "foo"),
            _ => panic!("expected SubDecl"),
        }
    }

    #[test]
    fn parse_subroutine_with_prototype() {
        let p = parse_ok("sub foo ($$) { 1 }");
        assert_eq!(p.statements.len(), 1);
        match &p.statements[0].kind {
            StmtKind::SubDecl { prototype, .. } => {
                assert!(prototype.is_some());
            }
            _ => panic!("expected SubDecl"),
        }
    }

    #[test]
    fn parse_anonymous_fn() {
        let p = parse_ok("my $f = fn { 1 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_if_statement() {
        let p = parse_ok("if (1) { 2 }");
        assert_eq!(p.statements.len(), 1);
        matches!(&p.statements[0].kind, StmtKind::If { .. });
    }

    #[test]
    fn parse_if_elsif_else() {
        let p = parse_ok("if (0) { 1 } elsif (1) { 2 } else { 3 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_unless_statement() {
        let p = parse_ok("unless (0) { 1 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_while_loop() {
        let p = parse_ok("while ($x) { $x-- }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_until_loop() {
        let p = parse_ok("until ($x) { $x++ }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_for_c_style() {
        let p = parse_ok("for (my $i=0; $i<10; $i++) { 1 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_foreach_loop() {
        let p = parse_ok("foreach my $x (@arr) { 1 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_loop_with_label() {
        let p = parse_ok("OUTER: for my $i (1..10) { last OUTER }");
        assert_eq!(p.statements.len(), 1);
        assert_eq!(p.statements[0].label.as_deref(), Some("OUTER"));
    }

    #[test]
    fn parse_begin_block() {
        let p = parse_ok("BEGIN { 1 }");
        assert_eq!(p.statements.len(), 1);
        matches!(&p.statements[0].kind, StmtKind::Begin(_));
    }

    #[test]
    fn parse_end_block() {
        let p = parse_ok("END { 1 }");
        assert_eq!(p.statements.len(), 1);
        matches!(&p.statements[0].kind, StmtKind::End(_));
    }

    #[test]
    fn parse_package_statement() {
        let p = parse_ok("package Foo::Bar");
        assert_eq!(p.statements.len(), 1);
        match &p.statements[0].kind {
            StmtKind::Package { name } => assert_eq!(name, "Foo::Bar"),
            _ => panic!("expected Package"),
        }
    }

    #[test]
    fn parse_use_statement() {
        let p = parse_ok("use strict");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_no_statement() {
        let p = parse_ok("no warnings");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_require_bareword() {
        let p = parse_ok("require Foo::Bar");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_require_string() {
        let p = parse_ok(r#"require "foo.pl""#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_eval_block() {
        let p = parse_ok("eval { 1 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_eval_string() {
        let p = parse_ok(r#"eval "1 + 2""#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_qw_word_list() {
        let p = parse_ok("my @a = qw(foo bar baz)");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_q_string() {
        let p = parse_ok("my $s = q{hello}");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_qq_string() {
        let p = parse_ok(r#"my $s = qq(hello $x)"#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_regex_match() {
        let p = parse_ok(r#"$x =~ /foo/"#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_regex_substitution() {
        let p = parse_ok(r#"$x =~ s/foo/bar/g"#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_transliterate() {
        let p = parse_ok(r#"$x =~ tr/a-z/A-Z/"#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_ternary_operator() {
        let p = parse_ok("my $x = $a ? 1 : 2");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_arrow_method_call() {
        let p = parse_ok("$obj->method()");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_arrow_deref_hash() {
        let p = parse_ok("$r->{key}");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_arrow_deref_array() {
        let p = parse_ok("$r->[0]");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_chained_arrow_deref() {
        let p = parse_ok("$r->{a}[0]{b}");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_my_multiple_vars() {
        let p = parse_ok("my ($a, $b, $c) = (1, 2, 3)");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_our_scalar() {
        let p = parse_ok("our $VERSION = '1.0'");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_local_scalar() {
        let p = parse_ok("local $/ = undef");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_state_variable() {
        let p = parse_ok("sub my_counter { state $n = 0; $n++ }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_postfix_if() {
        let p = parse_ok("print 1 if $x");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_postfix_unless() {
        let p = parse_ok("die 'error' unless $ok");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_postfix_while() {
        let p = parse_ok("$x++ while $x < 10");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_postfix_for() {
        let p = parse_ok("print for @arr");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_last_next_redo() {
        let p = parse_ok("for (@a) { next if $_ < 0; last if $_ > 10 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_return_statement() {
        let p = parse_ok("sub foo { return 42 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_wantarray() {
        let p = parse_ok("sub foo { wantarray ? @a : $a }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_caller_builtin() {
        let p = parse_ok("my @c = caller");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_ref_to_array() {
        let p = parse_ok("my $r = \\@arr");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_ref_to_hash() {
        let p = parse_ok("my $r = \\%hash");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_ref_to_scalar() {
        let p = parse_ok("my $r = \\$x");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_deref_scalar() {
        let p = parse_ok("my $v = $$r");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_deref_array() {
        let p = parse_ok("my @a = @$r");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_deref_hash() {
        let p = parse_ok("my %h = %$r");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_blessed_ref() {
        let p = parse_ok("bless $r, 'Foo'");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_heredoc_basic() {
        let p = parse_ok("my $s = <<END;\nfoo\nEND");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_heredoc_quoted() {
        let p = parse_ok("my $s = <<'END';\nfoo\nEND");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_do_block() {
        let p = parse_ok("my $x = do { 1 + 2 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_do_file() {
        let p = parse_ok(r#"do "foo.pl""#);
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_map_expression() {
        let p = parse_ok("my @b = map { $_ * 2 } @a");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_grep_expression() {
        let p = parse_ok("my @b = grep { $_ > 0 } @a");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_sort_expression() {
        let p = parse_ok("my @b = sort { $a <=> $b } @a");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_pipe_forward() {
        let p = parse_ok("@a |> map { $_ * 2 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_expression_from_str_simple() {
        let e = parse_expression_from_str("$x + 1", "-e").unwrap();
        assert!(matches!(e.kind, ExprKind::BinOp { .. }));
    }

    #[test]
    fn parse_expression_from_str_extra_tokens_error() {
        let err = parse_expression_from_str("$x; $y", "-e").unwrap_err();
        assert!(err.message.contains("Extra tokens"));
    }

    #[test]
    fn parse_slice_indices_from_str_basic() {
        let indices = parse_slice_indices_from_str("0, 1, 2", "-e").unwrap();
        assert_eq!(indices.len(), 3);
    }

    #[test]
    fn parse_format_value_line_empty() {
        let exprs = parse_format_value_line("").unwrap();
        assert!(exprs.is_empty());
    }

    #[test]
    fn parse_format_value_line_single() {
        let exprs = parse_format_value_line("$x").unwrap();
        assert_eq!(exprs.len(), 1);
    }

    #[test]
    fn parse_format_value_line_multiple() {
        let exprs = parse_format_value_line("$a, $b, $c").unwrap();
        assert_eq!(exprs.len(), 3);
    }

    #[test]
    fn parse_unclosed_brace_error() {
        let err = parse_err("sub foo {");
        assert!(!err.is_empty());
    }

    #[test]
    fn parse_unclosed_paren_error() {
        let err = parse_err("print (1, 2");
        assert!(!err.is_empty());
    }

    #[test]
    fn parse_invalid_statement_error() {
        let err = parse_err("???");
        assert!(!err.is_empty());
    }

    #[test]
    fn merge_expr_list_single() {
        let e = Expr {
            kind: ExprKind::Integer(1),
            line: 1,
        };
        let merged = merge_expr_list(vec![e.clone()]);
        matches!(merged.kind, ExprKind::Integer(1));
    }

    #[test]
    fn merge_expr_list_multiple() {
        let e1 = Expr {
            kind: ExprKind::Integer(1),
            line: 1,
        };
        let e2 = Expr {
            kind: ExprKind::Integer(2),
            line: 1,
        };
        let merged = merge_expr_list(vec![e1, e2]);
        matches!(merged.kind, ExprKind::List(_));
    }
}
