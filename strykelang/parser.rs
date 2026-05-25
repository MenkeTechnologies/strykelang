use crate::ast::*;
use crate::error::{ErrorKind, StrykeError, StrykeResult};
use crate::lexer::{Lexer, LITERAL_AT_IN_DQUOTE, LITERAL_DOLLAR_IN_DQUOTE};
use crate::token::Token;
use crate::vm_helper::VMHelper;

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
        "oursync" => StmtKind::OurSync(decls),
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
    /// When > 0 we are parsing inside a `{ … }` block (function body, `map`/`grep`,
    /// `for`, `if`, anonymous coderef, etc.). Inside any block, bare `_` is a topic
    /// reference (`$_[0]`/`$_`), so `my $i = _` means "capture the topic" and must
    /// NOT be auto-wrapped as an implicit zero-arg coderef. Only at the true top
    /// level (depth 0 — module scope) is `_` unbound, allowing `my $f = _ * 2` to
    /// parse as `my $f = fn { _ * 2 }`. Bumped in [`Self::parse_block`].
    block_depth: u32,
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
    /// Source path for [`StrykeError`] (matches lexer / `parse_with_file`).
    error_file: String,
    /// User-declared sub names (for allowing UDF to shadow stryke extensions in compat mode).
    declared_subs: std::collections::HashSet<String>,
    /// When > 0, `parse_named_expr` will not consume following barewords as paren-less
    /// function arguments. Used by thread macro to prevent `t Color::Red p` from
    /// interpreting `p` as an argument to the enum constructor instead of a stage.
    suppress_parenless_call: u32,
    /// Pre-built input expression for the next `parse_thread_macro_inner`
    /// call. Used by `~p>` continuation parsing (`||>` / `|then|`) to
    /// thread the par_reduce result into a normal `~>` continuation
    /// without re-parsing a source expression.
    pending_thread_input: Option<Expr>,
    /// When > 0, `parse_multiplication` will not consume `Token::Slash` as division.
    /// Used by thread macro so `/pattern/` is left for the stage parser to handle.
    suppress_slash_as_div: u32,
    /// When > 0, the lexer should not interpret `m/`, `s/`, etc. as regex-starters.
    /// Used by thread macro to prevent `/m/` from being misparsed.
    pub suppress_m_regex: u32,
    /// When > 0, `parse_range` will not consume `:` as the short-form range operator.
    /// Bumped while parsing the then-branch of a ternary `? :` so `a ? b : c` doesn't
    /// misparse `b : c` as a range.
    suppress_colon_range: u32,
    /// Counter (depth-tracked like [`Self::suppress_colon_range`]) that
    /// disables `~` as a range separator. Used inside paired `~...~` char-
    /// index/slice subscripts so the closing `~` doesn't get eaten as a
    /// range op. `:` range is still allowed inside (e.g. `$_~1:3~` is a
    /// slice with a `:` range as the index).
    suppress_tilde_range: u32,
    /// When true, `pipe_forward_apply` uses thread-last semantics (append to args)
    /// instead of thread-first (prepend). Set by `->>` thread macro.
    thread_last_mode: bool,
    /// When true, we're parsing a module (via `use`/`require`), not user code.
    /// Modules are allowed to shadow builtins; user code is not (unless `--compat`).
    pub parsing_module: bool,
    /// `self.pos` immediately after consuming a paren-list close (`(EXPR)`,
    /// `(EXPR, …)`, `()`) or `qw(…)` in `parse_primary`. The `x` operator
    /// reads this at parse time to distinguish `(LIST) x N` (list repetition)
    /// from `EXPR x N` (scalar string repetition). The compare is exact: any
    /// postfix consumption (`->method()`, `[idx]`, …) advances `self.pos`
    /// past this checkpoint, so list-repeat fires only when `x` is the very
    /// next token after the closing paren.
    list_construct_close_pos: Option<usize>,
    /// Synthetic SubDecl statements queued by anonymous-sub overload handlers
    /// (`use overload "+" => sub { ... }`) — drained at the end of
    /// [`Self::parse_program`] and prepended to the top-level statements so
    /// the package-qualified synthetic name resolves at runtime. (PARITY-012)
    pending_synthetic_subs: Vec<Statement>,
    /// Counter for unique anonymous-overload-handler names.
    next_overload_anon_id: u32,
    /// Token-vector indices where the lexer emitted a *bare* positional alias
    /// (`_`, `_0`, `_1`, …) — i.e. without a leading `$` sigil. Populated by
    /// [`crate::lexer::Lexer::tokenize`]. Consulted by [`Self::parse_my_our_local`]
    /// to auto-wrap an RHS expression that contains free positional aliases
    /// into an implicit zero-arg coderef, so `my $f = _ * 2` ≡
    /// `my $f = fn { _ * 2 }`.
    pub bare_positional_indices: std::collections::HashSet<usize>,
    /// Current package context — updated by `parse_package`. Defaults to
    /// `"main"`. Used by [`Self::check_udf_shadows_builtin`] to allow
    /// `fn name(...)` inside `package Foo` to shadow stryke builtins
    /// (the bare `name` becomes `Foo::name`, so the builtin remains
    /// reachable via the unqualified call from outside the package).
    current_package: String,
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
            pending_thread_input: None,
            suppress_slash_as_div: 0,
            suppress_m_regex: 0,
            suppress_colon_range: 0,
            suppress_tilde_range: 0,
            thread_last_mode: false,
            pending_synthetic_subs: Vec::new(),
            next_overload_anon_id: 0,
            parsing_module: false,
            list_construct_close_pos: None,
            bare_positional_indices: std::collections::HashSet::new(),
            block_depth: 0,
            current_package: "main".to_string(),
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
    /// A newline after the builtin name also terminates the pipe stage (implicit semicolon).
    fn pipe_supplies_slurped_list_operand(&self) -> bool {
        self.in_pipe_rhs()
            && (matches!(
                self.peek(),
                Token::Semicolon
                    | Token::RBrace
                    | Token::RParen
                    | Token::Eof
                    | Token::Comma
                    | Token::PipeForward
            ) || self.peek_line() > self.prev_line())
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

    /// List builtins that take `{ BLOCK }, LIST` and accept the threaded list at
    /// `args[1]` via [`Self::pipe_forward_apply`]. Used by both the pipe-forward
    /// dispatcher and `parse_thread_stage_with_block` so `~> @a NAME { ... }` and
    /// `@a |> NAME { ... }` route through the same substitution.
    fn is_block_then_list_pipe_builtin(name: &str) -> bool {
        matches!(
            name,
            "pfirst"
                | "pany"
                | "any"
                | "all"
                | "none"
                | "first"
                | "find_index"
                | "firstidx"
                | "first_index"
                | "take_while"
                | "drop_while"
                | "skip_while"
                | "reject"
                | "grepv"
                | "tap"
                | "peek"
                | "group_by"
                | "chunk_by"
                | "partition"
                | "min_by"
                | "max_by"
                | "zip_with"
                | "count_by"
        )
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
    fn parse_assign_expr_stop_at_pipe(&mut self) -> StrykeResult<Expr> {
        self.no_pipe_forward_depth = self.no_pipe_forward_depth.saturating_add(1);
        let r = self.parse_assign_expr();
        self.no_pipe_forward_depth = self.no_pipe_forward_depth.saturating_sub(1);
        r
    }

    fn syntax_err(&self, message: impl Into<String>, line: usize) -> StrykeError {
        StrykeError::new(ErrorKind::Syntax, message, line, self.error_file.clone())
    }

    /// Coderef-in-block-position helper for tier-2 list builtins (`any`,
    /// `all`, `none`, `first`, `take_while`, …). Returns `Some([f, list])`
    /// when the next tokens look like `$f [,] LIST` (or `$f` alone in
    /// pipe-RHS); `None` when the caller should fall through to the block
    /// form. The first arg is any coderef-shaped expression — runtime
    /// checks `as_code_ref()` and dispatches.
    fn try_parse_coderef_listop_args(&mut self, line: usize) -> StrykeResult<Option<Vec<Expr>>> {
        if !matches!(self.peek(), Token::ScalarVar(_) | Token::Backslash) {
            return Ok(None);
        }
        let f = self.parse_assign_expr_stop_at_pipe()?;
        let _ = self.eat(&Token::Comma);
        let list = if self.in_pipe_rhs()
            && matches!(
                self.peek(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
            ) {
            self.pipe_placeholder_list(line)
        } else {
            self.parse_expression()?
        };
        Ok(Some(vec![f, list]))
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

    fn expect(&mut self, expected: &Token) -> StrykeResult<usize> {
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
                // stryke-specific declaration keywords that start a new
                // statement on a fresh line. Without these, a bare `use
                // strict` / `use warnings` followed by `fn foo { ... }`
                // on the next line swallows `foo` as an import argument.
                | "fn" | "class" | "abstract" | "final" | "trait"
                | "state" | "mysync" | "oursync"
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

    pub fn parse_program(&mut self) -> StrykeResult<Program> {
        let mut statements = self.parse_statements()?;
        // Prepend any synthetic SubDecl stubs queued by anonymous overload
        // handlers so the package-qualified synthetic names resolve when the
        // overload table is consulted at runtime. (PARITY-012)
        if !self.pending_synthetic_subs.is_empty() {
            let synthetics = std::mem::take(&mut self.pending_synthetic_subs);
            let mut combined = Vec::with_capacity(synthetics.len() + statements.len());
            combined.extend(synthetics);
            combined.append(&mut statements);
            statements = combined;
        }
        Ok(Program { statements })
    }

    /// Parse statements until EOF. Used by parse_program and parse_block_from_str.
    pub fn parse_statements(&mut self) -> StrykeResult<Vec<Statement>> {
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

    fn parse_statement(&mut self) -> StrykeResult<Statement> {
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
                "sub" => {
                    if crate::no_interop_mode() {
                        return Err(self.syntax_err(
                            "stryke uses `fn` instead of `sub` (--no-interop is active)",
                            self.peek_line(),
                        ));
                    }
                    self.parse_sub_decl(true)?
                }
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
                "oursync" => {
                    if crate::compat_mode() {
                        return Err(self.syntax_err(
                            "`oursync` is a stryke extension (disabled by --compat)",
                            self.peek_line(),
                        ));
                    }
                    self.parse_my_our_local("oursync", false)?
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
                            // Accept type annotations the same way `typed
                            // my $x : Int` does — `const`/`frozen` is
                            // orthogonal to typing, and `: Type` after a
                            // name is unambiguous in either form.
                            let mut stmt = self.parse_my_our_local("my", true)?;
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
                    let lbl = self.try_take_loop_label();
                    let stmt = Statement {
                        label: None,
                        kind: StmtKind::Last(lbl.or(label.clone())),
                        line,
                    };
                    self.parse_stmt_postfix_modifier(stmt)?
                }
                "next" => {
                    self.advance();
                    let lbl = self.try_take_loop_label();
                    let stmt = Statement {
                        label: None,
                        kind: StmtKind::Next(lbl.or(label.clone())),
                        line,
                    };
                    self.parse_stmt_postfix_modifier(stmt)?
                }
                "redo" => {
                    self.advance();
                    let lbl = self.try_take_loop_label();
                    let stmt = Statement {
                        label: None,
                        kind: StmtKind::Redo(lbl.or(label.clone())),
                        line,
                    };
                    self.parse_stmt_postfix_modifier(stmt)?
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
                "before"
                    if matches!(
                        self.peek_at(1),
                        Token::SingleString(_) | Token::DoubleString(_)
                    ) =>
                {
                    self.parse_advice_decl(crate::ast::AdviceKind::Before)?
                }
                "after"
                    if matches!(
                        self.peek_at(1),
                        Token::SingleString(_) | Token::DoubleString(_)
                    ) =>
                {
                    self.parse_advice_decl(crate::ast::AdviceKind::After)?
                }
                "around"
                    if matches!(
                        self.peek_at(1),
                        Token::SingleString(_) | Token::DoubleString(_)
                    ) =>
                {
                    self.parse_advice_decl(crate::ast::AdviceKind::Around)?
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

    /// Consume an immediately-following loop label after `next`/`last`/`redo`.
    /// Matches identifiers that look like Perl loop labels — uppercase letters,
    /// digits, or `_`, and must start with a non-digit. Anything else
    /// (lowercase function names, `if`, `unless`, `(EXPR`, …) is left for the
    /// `EXPR`-form / postfix-modifier paths.
    fn try_take_loop_label(&mut self) -> Option<String> {
        let Token::Ident(s) = self.peek() else {
            return None;
        };
        let mut chars = s.chars();
        let first = chars.next()?;
        if !(first.is_ascii_uppercase() || first == '_') {
            return None;
        }
        let ok = s
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if !ok {
            return None;
        }
        let (Token::Ident(l), _) = self.advance() else {
            unreachable!()
        };
        Some(l)
    }

    /// Handle postfix if/unless on statement-level keywords like last/next.
    fn parse_stmt_postfix_modifier(&mut self, stmt: Statement) -> StrykeResult<Statement> {
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
                "pmap" | "pflat_map" | "pgrep" | "pfor" | "preduce" | "pcache" | "par" => {
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
                        "par" => ExprKind::ParExpr { block, list },
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
    fn stmt_into_parallel_block(&self, stmt: Statement) -> StrykeResult<Block> {
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
    /// `while` / `until` / `for` / `foreach` (mirrors `do { }` → [`ExprKind::Do`]\([`ExprKind::CodeRef`]\)).
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

    /// Token classes whose precedence sits below a Perl-style named unary
    /// operator. When one of these is the next token after a unary keyword
    /// (`length`, `len`, `cnt`, …), the keyword takes no explicit argument
    /// and the surrounding expression continues. Mirrors the `parse_one_arg_or_default`
    /// boundary set; kept as a separate predicate so other parse paths can
    /// reuse it without committing to default-to-`$_` semantics.
    fn peek_is_named_unary_terminator(&self) -> bool {
        matches!(
            self.peek(),
            Token::Semicolon
                | Token::RBrace
                | Token::RParen
                | Token::RBracket
                | Token::Eof
                | Token::Comma
                | Token::FatArrow
                | Token::PipeForward
                | Token::Question
                | Token::Colon
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
                | Token::Range
                | Token::RangeExclusive
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

    fn maybe_postfix_modifier(&mut self, expr: Expr) -> StrykeResult<Statement> {
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

    /// Map an operator-keyword token (the lexer converts `eq`, `ne`, …, `and`,
    /// `or`, `not`, `x` to dedicated tokens) back to its identifier spelling.
    /// Used in hash-key contexts where the bareword form is the user's intent.
    pub(crate) fn operator_keyword_to_ident_str(tok: &Token) -> Option<&'static str> {
        Some(match tok {
            Token::StrEq => "eq",
            Token::StrNe => "ne",
            Token::StrLt => "lt",
            Token::StrGt => "gt",
            Token::StrLe => "le",
            Token::StrGe => "ge",
            Token::StrCmp => "cmp",
            Token::LogAndWord => "and",
            Token::LogOrWord => "or",
            Token::LogNotWord => "not",
            Token::X => "x",
            _ => return None,
        })
    }

    /// Bare names that resolve to the topic-slot scalar matrix:
    /// `_`, `_0`, `_1`, …, `_N`, plus `_<+`, `_N<+` for the 4-deep outer chain.
    /// These must NOT be treated as zero-arg sub calls — they're scalar var refs.
    pub(crate) fn is_underscore_topic_slot(name: &str) -> bool {
        if name == "_" {
            return true;
        }
        if !name.starts_with('_') || name.len() < 2 {
            return false;
        }
        let bytes = name.as_bytes();
        let mut i = 1;
        // Optional digit run (positional slot index).
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        // Then any number of `<` chevrons (runtime cap at 5; lexer accepts more).
        let chevrons_start = i;
        while i < bytes.len() && bytes[i] == b'<' {
            i += 1;
        }
        // Must be one of: `_`, `_N`, `_<+`, `_N<+`. No other trailing chars.
        i == bytes.len() && (i > 1 || chevrons_start > 1)
    }

    /// Bareword names that map to Perl special variables / filehandles /
    /// compile-time tokens. A user-defined sub with any of these names
    /// would shadow the special variable's expression-position usage and
    /// produce silently-broken code. Reject at parse time with a
    /// foot-gun error message.
    ///
    /// Sigil-form spellings (`$@`, `$!`, `@ARGV`, `%ENV`, etc.) are caught
    /// separately via the `parse_sub_decl` catch-all branch — those don't
    /// even lex as `Token::Ident` so they hit a different code path.
    pub(crate) fn is_reserved_special_var_name(name: &str) -> bool {
        matches!(
            name,
            // Standard filehandles (Perl: STDIN, STDOUT, STDERR, ARGV, …)
            "STDIN" | "STDOUT" | "STDERR" | "ARGV" | "ARGVOUT" | "DATA"
            // Package globals, normally accessed via sigils (@ARGV, %ENV,
            // @INC, %SIG, @ISA, %ENV, etc.) — bareword shadow is a foot-gun.
            // NOTE: `AUTOLOAD` is intentionally NOT in this list — `fn
            // AUTOLOAD { ... }` is the legitimate Perl idiom for handling
            // missing-method dispatch. The runtime sets `$AUTOLOAD` to the
            // missing sub's qualified name before invoking the user's
            // AUTOLOAD sub. Adding it here would break that mechanism.
            | "ENV" | "INC" | "SIG" | "ISA"
            | "EXPORT" | "EXPORT_OK" | "EXPORT_TAGS"
            | "VERSION"
            // Compile-time tokens (resolve to constants at parse time).
            | "__FILE__" | "__LINE__" | "__PACKAGE__" | "__SUB__"
            | "__DATA__" | "__END__"
        )
    }

    /// Identifiers that start a [`parse_named_expr`] arm (builtins / special forms), not a bare sub call.
    fn bareword_stmt_may_be_sub(name: &str) -> bool {
        // Topic-slot scalar names (`_`, `_N`, `_<+`, `_N<+`) are scalar
        // variables, not zero-arg sub calls. Without this guard, the
        // statement-position parser would emit `Op::Call("_0", 0)` and fail
        // at runtime with "Undefined subroutine &_0".
        if Self::is_underscore_topic_slot(name) {
            return false;
        }
        !matches!(
            name,
            "__FILE__"
                | "__LINE__"
                | "__PACKAGE__"
                | "__SUB__"
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
                | "pfrequencies"
                | "pfreq"
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
                | "par"
                | "par_lines"
                | "par_walk"
                | "pipe"
                | "pipes"
                | "block_devices"
                | "char_devices"
                | "exe"
                | "executables"
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
                | "par_reduce"
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
                | "find_index"
                | "firstidx"
                | "first_index"
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
                | "swallow"
                | "sockets"
                | "sort"
                | "splice"
                | "splice_last"
                | "splice1"
                | "spl_last"
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

    fn parse_block(&mut self) -> StrykeResult<Block> {
        self.expect(&Token::LBrace)?;
        // Statements inside a block are NOT pipe RHS - reset depth so nested `~>`
        // parses its own input instead of using `$_[0]` placeholder.
        let saved_pipe_rhs_depth = self.pipe_rhs_depth;
        self.pipe_rhs_depth = 0;
        self.block_depth += 1;
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
        self.pipe_rhs_depth = saved_pipe_rhs_depth;
        self.block_depth -= 1;
        Self::default_topic_for_sole_bareword(&mut stmts);
        Ok(stmts)
    }

    /// Try to parse `|$var1, $var2, ...|` at the start of a block.
    /// Returns `None` if the leading `|` is not block-param syntax.
    /// When successful, returns `my $var = <implicit>` assignment statements
    /// that alias the block's positional arguments.
    fn try_parse_block_params(&mut self) -> StrykeResult<Option<Vec<Statement>>> {
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
                        list_context: false,
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
                        list_context: false,
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
    /// Desugars to a `defer__internal(fn { BLOCK })` function call that the compiler
    /// handles specially by emitting Op::DeferBlock.
    fn parse_defer_stmt(&mut self) -> StrykeResult<Statement> {
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
    fn parse_try_catch(&mut self) -> StrykeResult<Statement> {
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
    /// When `thread_last` is true (`->>` syntax), injects as last arg instead of first.
    ///
    /// When invoked as the RHS of `|>` (e.g. `LHS |> t s1 s2 ...`), the init
    /// is not parsed from tokens — using `parse_unary()` there lets the first
    /// bareword greedily consume the next token as its arg, which misparses
    /// `t inc pow($_, 2) p` as init=`inc(pow(…))` + stage=`p` instead of three
    /// separate stages. Instead, seed init with `$_[0]`, run every remaining
    /// token through the stage loop, and wrap the resulting chain in a
    /// `CodeRef`. The outer `pipe_forward_apply` then calls it with `lhs` as
    /// `$_[0]`, giving `LHS |> t s1 s2 s3` == `LHS |> s1 |> s2 |> s3`.
    fn parse_thread_macro(&mut self, _line: usize, thread_last: bool) -> StrykeResult<Expr> {
        self.parse_thread_macro_inner(_line, thread_last, None)
    }

    /// Shared core for `~>` / `~>>` / `~s>` / `~s>>`. When
    /// `parallel_collector` is `Some` (streaming-mode entry from `~s>` /
    /// `~s>>`), after each stage is parsed we push the (just-built) stage
    /// expression into the collector and reset `result` to `$_` so the
    /// next stage parses against a fresh topic. The collector ends up
    /// with one Expr per stage where each stage's input is `$_`, ready
    /// to be wrapped as a `fn { ... }` closure for the per-item
    /// streaming runtime (`_thread_par_run`).
    fn parse_thread_macro_inner(
        &mut self,
        _line: usize,
        thread_last: bool,
        mut parallel_collector: Option<&mut Vec<Expr>>,
    ) -> StrykeResult<Expr> {
        // Set thread-last mode for pipe_forward_apply calls within this macro
        let saved_thread_last = self.thread_last_mode;
        self.thread_last_mode = thread_last;

        let pipe_rhs_wrap = self.in_pipe_rhs();
        // `pending_thread_input` (set by `~p>` continuation parsing after
        // `||>` / `|then|`) supplies a pre-built input expression so we
        // skip parsing a source.
        let mut result = if let Some(pre) = self.pending_thread_input.take() {
            pre
        } else if pipe_rhs_wrap {
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
        // Capture the source expression for parallel mode BEFORE any stage
        // is parsed, then reset `result` to `$_` so the first stage's parse
        // reads the topic instead of the source.
        let source_for_par = if parallel_collector.is_some() {
            let src = std::mem::replace(
                &mut result,
                Expr {
                    kind: ExprKind::ScalarVar("_".into()),
                    line: _line,
                },
            );
            Some(src)
        } else {
            None
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
                // `||>` (LogOr + NumGt): chunk-parallel → sequential boundary
                // for `~p>` macros. Other thread macros never see this in
                // practice; if it appears, terminate the macro and let the
                // outer parser handle it.
                Token::LogOr if matches!(self.peek_at(1), Token::NumGt) => break,
                // `|then|` (BitOr + Ident("then") + BitOr): same boundary.
                Token::BitOr
                    if matches!(self.peek_at(1), Token::Ident(ref n) if n == "then")
                        && matches!(self.peek_at(2), Token::BitOr) =>
                {
                    break
                }
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
                // `sub { block }` — blocked in no-interop mode
                Token::Ident(ref name) if name == "sub" => {
                    if crate::no_interop_mode() {
                        return Err(self.syntax_err(
                            "stryke uses `fn {}` instead of `sub {}` (--no-interop)",
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
                    self.parse_sub_attributes()?;
                    let body = self.parse_fn_eq_body_or_block(false)?;
                    let code_ref = Expr {
                        kind: ExprKind::CodeRef { params, body },
                        line: stage_line,
                    };
                    result = self.pipe_forward_apply(result, code_ref, stage_line)?;
                }
                // `ident` possibly followed by block (or namespaced like `Foo::Bar::func`)
                Token::Ident(ref name) => {
                    let mut func_name = name.clone();
                    self.advance();

                    // Collect namespaced function name (e.g., Rosetta::Stack::push)
                    while matches!(self.peek(), Token::PackageSep) {
                        self.advance(); // consume `::`
                        if let Token::Ident(ref part) = self.peek().clone() {
                            func_name.push_str("::");
                            func_name.push_str(part);
                            self.advance();
                        } else {
                            return Err(self.syntax_err(
                                format!(
                                    "Expected identifier after `::` in thread stage, got {:?}",
                                    self.peek()
                                ),
                                stage_line,
                            ));
                        }
                    }

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
                    // `par_reduce { extract } [ { merge } ]` — chunk-extract-merge.
                    // First block runs per chunk in parallel; optional second
                    // block reduces pairwise across chunks (omit for auto-merge
                    // by result type).
                    } else if func_name == "par_reduce" {
                        let extract_block = self.parse_block_or_bareword_block()?;
                        let reduce_block = if matches!(self.peek(), Token::LBrace) {
                            Some(self.parse_block()?)
                        } else {
                            None
                        };
                        let placeholder = self.pipe_placeholder_list(stage_line);
                        let stage = Expr {
                            kind: ExprKind::ParReduceExpr {
                                extract_block,
                                reduce_block,
                                list: Box::new(placeholder),
                            },
                            line: stage_line,
                        };
                        result = self.pipe_forward_apply(result, stage, stage_line)?;
                    // `pmap_on $cluster { BLOCK }` — parallel map dispatched to a remote
                    // cluster. Mirrors the `pmap_chunked` thread-stage shape; the cluster
                    // expression is parsed before the block, the threaded list slots in
                    // as the placeholder.
                    } else if func_name == "pmap_on" || func_name == "pflat_map_on" {
                        // Suppress `$cluster { ... }` auto-arrow (`$h->{...}`) so the
                        // brace opens the block, not a hash subscript.
                        self.suppress_scalar_hash_brace =
                            self.suppress_scalar_hash_brace.saturating_add(1);
                        let cluster = self.parse_assign_expr();
                        self.suppress_scalar_hash_brace =
                            self.suppress_scalar_hash_brace.saturating_sub(1);
                        let cluster = cluster?;
                        // Optional comma between cluster and block (matches the
                        // canonical `pmap_on $c, { BLOCK } @list` form in the LSP docs).
                        self.eat(&Token::Comma);
                        let block = self.parse_block_or_bareword_block()?;
                        let placeholder = self.pipe_placeholder_list(stage_line);
                        let stage = Expr {
                            kind: ExprKind::PMapExpr {
                                block,
                                list: Box::new(placeholder),
                                progress: None,
                                flat_outputs: func_name == "pflat_map_on",
                                on_cluster: Some(Box::new(cluster)),
                                stream: false,
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
                            // If no `$_` placeholder, auto-inject threaded value.
                            // Thread-first: `t data to_file("/tmp/o.html")` → `to_file($_, "/tmp/o.html")`
                            // Thread-last: `->> data to_file("/tmp/o.html")` → `to_file("/tmp/o.html", $_)`
                            if !call_args.iter().any(Self::expr_contains_topic_var) {
                                let topic = Expr {
                                    kind: ExprKind::ScalarVar("_".to_string()),
                                    line: stage_line,
                                };
                                if self.thread_last_mode {
                                    call_args.push(topic);
                                } else {
                                    call_args.insert(0, topic);
                                }
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
            // Parallel mode: each iteration of the loop has produced a
            // stage expression where `$_` is the input. Push it into the
            // collector and reset `result` to `$_` so the next stage
            // parses against a fresh topic.
            if let Some(stages) = parallel_collector.as_mut() {
                let stage_body = std::mem::replace(
                    &mut result,
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: stage_line,
                    },
                );
                stages.push(stage_body);
            }
        }

        // Restore thread-last mode
        self.thread_last_mode = saved_thread_last;

        // Parallel mode: lower to `_thread_par_run(source_expr, [stage_closures], thread_last)`.
        // The runtime treats the source value as a list-of-items and feeds
        // each item into stage 1 via a bounded channel. Each stage runs in
        // its own worker; stages are wrapped as `fn { body }` closures so
        // the runtime sets `$_` to the current item before invoking.
        if let Some(stages) = parallel_collector {
            let source_expr = source_for_par.unwrap_or(result);
            if stages.is_empty() {
                return Err(self.syntax_err(
                    "~p> / ~p>> require at least one stage after the source",
                    _line,
                ));
            }
            // Wrap each stage body in `[ ... ]` (an ArrayRef) so list-returning
            // ops like `map`/`grep` propagate their full output instead of
            // collapsing to a scalar count. The runtime worker peels one
            // level of array-ref via `map_flatten_outputs(true)` so each
            // element flows downstream as its own item.
            let stage_closures: Vec<Expr> = stages
                .drain(..)
                .map(|body| {
                    let body_line = body.line;
                    let wrapped = Expr {
                        kind: ExprKind::ArrayRef(vec![body]),
                        line: body_line,
                    };
                    Expr {
                        kind: ExprKind::CodeRef {
                            params: vec![],
                            body: vec![Statement {
                                label: None,
                                kind: StmtKind::Expression(wrapped),
                                line: body_line,
                            }],
                        },
                        line: body_line,
                    }
                })
                .collect();
            let stages_arr = Expr {
                kind: ExprKind::ArrayRef(stage_closures),
                line: _line,
            };
            let thread_last_flag = Expr {
                kind: ExprKind::Integer(if thread_last { 1 } else { 0 }),
                line: _line,
            };
            // Argument order: stages, thread_last, source... — source
            // is LAST so its list expansion (`(1,2,3)`, `@a`, ranges)
            // lands in the variadic tail. Pre-fix the source was first
            // and any list source flattened across the slot, breaking
            // the `args.len() == 3` invariant in `_thread_par_run` and
            // hitting "expected 3 args" for `~s> (1,2,3) sum` etc.
            return Ok(Expr {
                kind: ExprKind::FuncCall {
                    name: "_thread_par_run".into(),
                    args: vec![stages_arr, thread_last_flag, source_expr],
                },
                line: _line,
            });
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

    /// Walk tokens in `[rhs_start, rhs_end)` looking for a *free* bare
    /// topic-slot index (one in `self.bare_positional_indices` at
    /// brace-depth 0 within the RHS). Any `_` inside `{ ... }` is
    /// considered bound by whatever defines that block (closure,
    /// hash literal, map/grep/sort/match arm) and doesn't count.
    ///
    /// Special case: when the RHS *starts* with a thread-macro intro
    /// token (`~>`, `->>`, `~>>`, `~p>`, `~s>`, `~d>` and `-Last`
    /// variants), the macro itself binds `_` for all stage expressions
    /// — only the immediate input expression after the arrow can
    /// trigger the wrap. `->> 10 div(_, 2)` is eager (input = `10`,
    /// `_` is the threaded placeholder), but `~> _ uc` wraps (input
    /// is the free bare `_`).
    ///
    /// Drives the implicit-coderef sugar in `parse_my_our_local`.
    fn rhs_has_free_bare_topic_slot(&self, rhs_start: usize, rhs_end: usize) -> bool {
        let end = rhs_end.min(self.tokens.len());
        if rhs_start < end && Self::is_thread_arrow(&self.tokens[rhs_start].0) {
            // Only the input expression (first token after the arrow)
            // can trigger the wrap; everything else is a stage and
            // its bare `_` is the threaded placeholder.
            let input = rhs_start + 1;
            return input < end && self.bare_positional_indices.contains(&input);
        }
        let mut brace_depth = 0i32;
        for i in rhs_start..end {
            if brace_depth == 0 && self.bare_positional_indices.contains(&i) {
                return true;
            }
            match &self.tokens[i].0 {
                Token::LBrace | Token::ArrowBrace => brace_depth += 1,
                Token::RBrace => brace_depth -= 1,
                _ => {}
            }
        }
        false
    }

    fn is_thread_arrow(tok: &Token) -> bool {
        matches!(
            tok,
            Token::ThreadArrow
                | Token::ThreadArrowLast
                | Token::ThreadArrowStream
                | Token::ThreadArrowStreamLast
                | Token::ThreadArrowPar
                | Token::ThreadArrowParLast
                | Token::ThreadArrowDist
                | Token::ThreadArrowDistLast
        )
    }

    /// Apply a bare function name in thread context, handling unary builtins specially.
    fn thread_apply_bare_func(&self, name: &str, arg: Expr, line: usize) -> StrykeResult<Expr> {
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
            "rand" => ExprKind::Rand(Some(Box::new(arg))),
            "srand" => ExprKind::Srand(Some(Box::new(arg))),
            // Type/ref functions
            "defined" | "def" => ExprKind::Defined(Box::new(arg)),
            "ref" => ExprKind::Ref(Box::new(arg)),
            "scalar" => {
                if crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke uses `len` (also `cnt` / `count`) instead of `scalar` (--no-interop)",
                        line,
                    ));
                }
                ExprKind::ScalarContext(Box::new(arg))
            }
            // Array/hash functions
            "keys" => ExprKind::Keys(Box::new(arg)),
            "values" => ExprKind::Values(Box::new(arg)),
            "each" => ExprKind::Each(Box::new(arg)),
            "pop" => ExprKind::Pop(Box::new(arg)),
            "shift" => ExprKind::Shift(Box::new(arg)),
            "reverse" => {
                if crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke uses `rev` instead of `reverse` (--no-interop)",
                        line,
                    ));
                }
                ExprKind::ReverseExpr(Box::new(arg))
            }
            "reversed" | "rv" | "rev" => ExprKind::Rev(Box::new(arg)),
            "sort" | "so" => ExprKind::SortExpr {
                cmp: None,
                list: Box::new(arg),
            },
            "psort" => ExprKind::PSortExpr {
                cmp: None,
                list: Box::new(arg),
                progress: None,
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
            "pfrequencies" | "pfreq" | "pfrq" => ExprKind::FuncCall {
                name: "pfrequencies".to_string(),
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
            "swallow" | "swa" => ExprKind::Swallow(Box::new(arg)),
            "glob" => ExprKind::Glob(vec![arg]),
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
                if crate::no_interop_mode() {
                    return Err(
                        self.syntax_err("stryke uses `p` instead of `say` (--no-interop)", line)
                    );
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
    fn parse_thread_stage_with_block(&mut self, name: &str, line: usize) -> StrykeResult<Expr> {
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
            "par" => Ok(Expr {
                kind: ExprKind::ParExpr {
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
                // Generic: parse block and treat as FuncCall with code ref arg.
                // Block-then-list pipe builtins (`pfirst`, `any`, `take_while`, etc.)
                // need the threaded list slot pre-allocated at args[1] so
                // `pipe_forward_apply` can substitute the lhs there (parser.rs:5823).
                // For everything else, the generic pipe-forward arm prepends or
                // appends the lhs based on `thread_last_mode`.
                let code_ref = Expr {
                    kind: ExprKind::CodeRef {
                        params: vec![],
                        body: block,
                    },
                    line,
                };
                let args = if Self::is_block_then_list_pipe_builtin(name) {
                    vec![code_ref, placeholder]
                } else {
                    vec![code_ref]
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.to_string(),
                        args,
                    },
                    line,
                })
            }
        }
    }

    /// `tie %hash | tie @arr | tie $x , 'Class', ...args`
    fn parse_tie_stmt(&mut self) -> StrykeResult<Statement> {
        let line = self.peek_line();
        self.advance(); // tie
                        // `tie my $x, Class` and `tie our $x, Class` — common Perl idiom.
                        // Desugar by emitting an implicit `my $x` (or `our $x`) declaration
                        // before the tie. The tie target then references the just-declared
                        // variable. Without this, `tie my $x, Class, ARGS` errors with
                        // "tie expects $scalar, @array, or %hash, got Ident(\"my\")".
        let mut implicit_decl: Option<Statement> = None;
        if let Token::Ident(kw) = self.peek().clone() {
            if matches!(kw.as_str(), "my" | "our") {
                let kw_line = self.peek_line();
                self.advance(); // my / our
                                // Read the variable being declared (must be Scalar/Array/Hash).
                let (decl_sigil, decl_name) = match self.peek().clone() {
                    Token::ScalarVar(s) => (Sigil::Scalar, s),
                    Token::ArrayVar(a) => (Sigil::Array, a),
                    Token::HashVar(h) => (Sigil::Hash, h),
                    tok => {
                        return Err(self.syntax_err(
                            format!("expected variable after `tie {}`, got {:?}", kw, tok),
                            self.peek_line(),
                        ));
                    }
                };
                let decls = vec![VarDecl {
                    sigil: decl_sigil,
                    name: decl_name.clone(),
                    initializer: None,
                    frozen: false,
                    type_annotation: None,
                    list_context: false,
                }];
                implicit_decl = Some(Statement {
                    label: None,
                    kind: if kw == "my" {
                        StmtKind::My(decls)
                    } else {
                        StmtKind::Our(decls)
                    },
                    line: kw_line,
                });
                // Don't advance past the variable token here — fall through
                // to the existing match below so `target` is built from the
                // same token (the ScalarVar/ArrayVar/HashVar path will
                // advance and capture the name).
            }
        }
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
        let tie_stmt = Statement {
            label: None,
            kind: StmtKind::Tie {
                target,
                class,
                args,
            },
            line,
        };
        if let Some(decl) = implicit_decl {
            // Wrap the implicit `my $x` + tie in a `StmtGroup` so they live
            // in the same lexical block (the parser desugar is invisible to
            // callers; `StmtGroup` runs statements in order without a frame
            // push).
            Ok(Statement {
                label: None,
                kind: StmtKind::StmtGroup(vec![decl, tie_stmt]),
                line,
            })
        } else {
            Ok(tie_stmt)
        }
    }

    /// `given (EXPR) { ... }`
    fn parse_given(&mut self) -> StrykeResult<Statement> {
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
    fn parse_when_stmt(&mut self) -> StrykeResult<Statement> {
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
    fn parse_default_stmt(&mut self) -> StrykeResult<Statement> {
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

    /// `cond { EXPR => RESULT, ..., default => RESULT }`
    ///
    /// Desugars to an if/elsif/else chain at parse time.
    /// Each arm is `condition => { body }` or `condition => expr`.
    /// `default => ...` becomes the else branch.
    fn parse_cond_expr(&mut self, line: usize) -> StrykeResult<Expr> {
        self.expect(&Token::LBrace)?;

        let mut arms: Vec<(Expr, Block)> = Vec::new();
        let mut else_block: Option<Block> = None;

        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            let arm_line = self.peek_line();

            // Check for `default =>`
            let is_default = matches!(self.peek(), Token::Ident(ref s) if s == "default")
                && matches!(self.peek_at(1), Token::FatArrow);

            if is_default {
                self.advance(); // consume `default`
                self.advance(); // consume `=>`
                let body = if matches!(self.peek(), Token::LBrace) {
                    self.parse_block()?
                } else {
                    let expr = self.parse_assign_expr()?;
                    vec![Statement {
                        label: None,
                        kind: StmtKind::Expression(expr),
                        line: arm_line,
                    }]
                };
                else_block = Some(body);
                self.eat(&Token::Comma);
                break; // default must be last
            }

            // Parse condition expression (stop before `=>`)
            let condition = self.parse_assign_expr()?;
            self.expect(&Token::FatArrow)?;

            let body = if matches!(self.peek(), Token::LBrace) {
                self.parse_block()?
            } else {
                let expr = self.parse_assign_expr()?;
                vec![Statement {
                    label: None,
                    kind: StmtKind::Expression(expr),
                    line: arm_line,
                }]
            };

            arms.push((condition, body));
            self.eat(&Token::Comma);
        }

        self.expect(&Token::RBrace)?;

        if arms.is_empty() {
            return Err(self.syntax_err("cond requires at least one condition arm", line));
        }

        // Build if/elsif/else chain from the arms.
        let (first_cond, first_body) = arms.remove(0);
        let elsifs: Vec<(Expr, Block)> = arms;

        // Wrap in a do-block so `cond { ... }` is an expression.
        let if_stmt = Statement {
            label: None,
            kind: StmtKind::If {
                condition: first_cond,
                body: first_body,
                elsifs,
                else_block,
            },
            line,
        };
        let inner = Expr {
            kind: ExprKind::CodeRef {
                params: vec![],
                body: vec![if_stmt],
            },
            line,
        };
        Ok(Expr {
            kind: ExprKind::Do(Box::new(inner)),
            line,
        })
    }

    /// `match (EXPR) { PATTERN => EXPR, ... }`
    fn parse_algebraic_match_expr(&mut self, line: usize) -> StrykeResult<Expr> {
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

    fn parse_match_pattern(&mut self) -> StrykeResult<MatchPattern> {
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
    fn parse_match_array_elems_until_rbracket(&mut self) -> StrykeResult<Vec<MatchArrayElem>> {
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

    fn parse_match_array_pattern(&mut self) -> StrykeResult<MatchPattern> {
        self.expect(&Token::LBracket)?;
        let elems = self.parse_match_array_elems_until_rbracket()?;
        Ok(MatchPattern::Array(elems))
    }

    fn parse_match_hash_pattern(&mut self) -> StrykeResult<MatchPattern> {
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
    fn parse_eval_timeout(&mut self) -> StrykeResult<Statement> {
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

    fn parse_if(&mut self) -> StrykeResult<Statement> {
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
    fn parse_if_let(&mut self, line: usize) -> StrykeResult<Statement> {
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

    fn parse_unless(&mut self) -> StrykeResult<Statement> {
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

    fn parse_while(&mut self) -> StrykeResult<Statement> {
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
    fn parse_while_let(&mut self, line: usize) -> StrykeResult<Statement> {
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
                list_context: false,
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

    fn parse_until(&mut self) -> StrykeResult<Statement> {
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
    fn parse_optional_continue_block(&mut self) -> StrykeResult<Option<Block>> {
        if let Token::Ident(ref kw) = self.peek().clone() {
            if kw == "continue" {
                self.advance();
                return Ok(Some(self.parse_block()?));
            }
        }
        Ok(None)
    }

    fn parse_for_or_foreach(&mut self) -> StrykeResult<Statement> {
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

    fn parse_c_style_for(&mut self, line: usize) -> StrykeResult<Statement> {
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

    fn parse_foreach(&mut self) -> StrykeResult<Statement> {
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

    fn parse_scalar_var_name(&mut self) -> StrykeResult<String> {
        match self.advance() {
            (Token::ScalarVar(name), _) => Ok(name),
            (tok, line) => {
                Err(self.syntax_err(format!("Expected scalar variable, got {:?}", tok), line))
            }
        }
    }

    /// After `(` was consumed: Perl5 prototype characters until `)` (or `$)` + `{`).
    fn parse_legacy_sub_prototype_tail(&mut self) -> StrykeResult<String> {
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

    fn parse_sub_signature_hash_key(&mut self) -> StrykeResult<String> {
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

    fn parse_sub_signature_param_list(&mut self) -> StrykeResult<Vec<SubSigParam>> {
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
    fn parse_sub_sig_or_prototype_opt(
        &mut self,
    ) -> StrykeResult<(Vec<SubSigParam>, Option<String>)> {
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
    fn parse_sub_attributes(&mut self) -> StrykeResult<()> {
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

    /// After `fn` + optional `(SIG)` + attrs: stryke-only `= EXPR` (one assign-level expression;
    /// no top-level `,` after the expression). Returns `None` if the next token is not `=`.
    fn try_parse_fn_assign_shorthand_body(&mut self) -> StrykeResult<Option<Block>> {
        if !self.eat(&Token::Assign) {
            return Ok(None);
        }
        let expr = self.parse_assign_expr()?;
        if matches!(self.peek(), Token::Comma) {
            return Err(self.syntax_err(
                "`fn ... =` allows only a single expression; use `fn ... { ... }` for multiple statements",
                self.peek_line(),
            ));
        }
        let eline = expr.line;
        self.eat(&Token::Semicolon);
        let mut body = vec![Statement {
            label: None,
            kind: StmtKind::Expression(expr),
            line: eline,
        }];
        Self::default_topic_for_sole_bareword(&mut body);
        Ok(Some(body))
    }

    /// After `fn` + optional `(SIG)` + attrs: `{ ... }` or stryke-only `= EXPR` (see
    /// [`Self::try_parse_fn_assign_shorthand_body`]). `sub` always requires `{ ... }`.
    fn parse_fn_eq_body_or_block(&mut self, is_sub_keyword: bool) -> StrykeResult<Block> {
        if !is_sub_keyword {
            if let Some(block) = self.try_parse_fn_assign_shorthand_body()? {
                return Ok(block);
            }
        }
        self.parse_block()
    }

    fn parse_sub_decl(&mut self, is_sub_keyword: bool) -> StrykeResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'sub' or 'fn'
        match self.peek().clone() {
            Token::Ident(_) => {
                let name = self.parse_package_qualified_identifier()?;
                // Topic-slot barewords (`_`, `_<`, `_<<`, `_<<<`, `_<<<<`,
                // `_0`, `_1`, …, `_N`, plus `_N<+` chain forms) are scalar
                // refs to the current/positional/outer topic. A user-defined
                // sub with any of these names — bare or package-qualified —
                // would shadow the topic in expression position and silently
                // break every `_`-aware builtin (`map { _ }`, `say _`,
                // `lc _`, …). Reject ALL forms at parse time, including
                // `Foo::_`, `Pkg::_0`, `My::Module::_<<<<`.
                let bare = name.rsplit("::").next().unwrap_or(&name);
                if Self::is_underscore_topic_slot(bare) {
                    return Err(self.syntax_err(
                        format!(
                            "`fn {}` would shadow the topic-slot scalar; pick a different name",
                            name
                        ),
                        line,
                    ));
                }
                if Self::is_reserved_special_var_name(bare) {
                    return Err(self.syntax_err(
                        format!(
                            "`fn {}` would shadow a Perl special variable / filehandle / compile-time token; pick a different name",
                            name
                        ),
                        line,
                    ));
                }
                // Allow shadowing builtins:
                // - In compat mode (full Perl 5)
                // - When parsing a module (imports should work)
                // Block shadowing:
                // - In user code (default mode, not parsing module)
                // - Always in no-interop mode
                let allow_shadow =
                    crate::compat_mode() || (self.parsing_module && !crate::no_interop_mode());
                if !allow_shadow {
                    self.check_udf_shadows_builtin(&name, line)?;
                }
                self.declared_subs.insert(name.clone());
                let (params, prototype) = self.parse_sub_sig_or_prototype_opt()?;
                self.parse_sub_attributes()?;
                let body = self.parse_fn_eq_body_or_block(is_sub_keyword)?;
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
                // In no-interop mode, `sub {}` anonymous is not allowed — must use `fn {}`
                if is_sub_keyword && crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke uses `fn {}` instead of `sub {}` (--no-interop)",
                        line,
                    ));
                }
                // Statement-level anonymous sub: `fn { }`, `sub () { }`, `sub :lvalue { }`
                let (params, _prototype) = self.parse_sub_sig_or_prototype_opt()?;
                self.parse_sub_attributes()?;
                let body = self.parse_fn_eq_body_or_block(is_sub_keyword)?;
                Ok(Statement {
                    label: None,
                    kind: StmtKind::Expression(Expr {
                        kind: ExprKind::CodeRef { params, body },
                        line,
                    }),
                    line,
                })
            }
            tok => {
                // Sigil-form topic-slot names (`fn $_`, `fn $_<`, `fn $_0`,
                // `fn @_`, `fn %_`, …) are also rejected with the same
                // foot-gun message as the bareword form. Without this branch
                // the user gets a confusing generic "Expected sub name" error.
                let topic_name = match &tok {
                    Token::ScalarVar(n) | Token::ArrayVar(n) | Token::HashVar(n)
                        if Self::is_underscore_topic_slot(n) =>
                    {
                        Some((
                            match &tok {
                                Token::ScalarVar(_) => '$',
                                Token::ArrayVar(_) => '@',
                                Token::HashVar(_) => '%',
                                _ => unreachable!(),
                            },
                            n.clone(),
                        ))
                    }
                    _ => None,
                };
                if let Some((sigil, n)) = topic_name {
                    return Err(self.syntax_err(
                        format!(
                            "`fn {}{}` would shadow the topic-slot scalar; pick a different name",
                            sigil, n
                        ),
                        self.peek_line(),
                    ));
                }
                // Sigil-form Perl special variables / globals — same foot-gun.
                // Catches `fn $@`, `fn $!`, `fn $/`, `fn $\\`, `fn $,`, `fn $;`,
                // `fn $"`, `fn $.`, `fn $0`, `fn $$`, `fn $?`, `fn $1`-`$9`,
                // `fn $^I`, `fn @ARGV`, `fn @INC`, `fn %ENV`, `fn %SIG`, etc.
                let special_var = match &tok {
                    Token::ScalarVar(n) | Token::ArrayVar(n) | Token::HashVar(n) => Some((
                        match &tok {
                            Token::ScalarVar(_) => '$',
                            Token::ArrayVar(_) => '@',
                            Token::HashVar(_) => '%',
                            _ => unreachable!(),
                        },
                        n.clone(),
                    )),
                    _ => None,
                };
                if let Some((sigil, n)) = special_var {
                    return Err(self.syntax_err(
                        format!(
                            "`fn {}{}` would shadow a Perl special variable / global; pick a different name",
                            sigil, n
                        ),
                        self.peek_line(),
                    ));
                }
                // After `fn`, `%` lexes as `Token::Percent` (modulo) rather
                // than a hash sigil — but `fn %ENV { }`, `fn %SIG { }`,
                // `fn %_ { }`, etc. all reach here. Emit the same foot-gun
                // message as the sigil-form catch above.
                if matches!(tok, Token::Percent) {
                    return Err(self.syntax_err(
                        "`fn %NAME` is not a valid sub declaration — `%name` would refer to a hash variable, not a sub name. To define a sub, use `fn NAME { ... }`",
                        self.peek_line(),
                    ));
                }
                Err(self.syntax_err(
                    format!("Expected sub name, `(`, `{{`, or `:`, got {:?}", tok),
                    self.peek_line(),
                ))
            }
        }
    }

    /// `before|after|around "<glob>" { ... }` — register AOP advice.
    /// The pattern is a glob (`*`, `?`) matched against the called sub's bare name.
    fn parse_advice_decl(&mut self, kind: crate::ast::AdviceKind) -> StrykeResult<Statement> {
        let line = self.peek_line();
        self.advance(); // before/after/around
        let pattern = match self.advance() {
            (Token::SingleString(s), _) | (Token::DoubleString(s), _) => s,
            (tok, err_line) => {
                return Err(self.syntax_err(
                    format!(
                        "Expected string-literal pattern after `{}`, got {:?}",
                        match kind {
                            crate::ast::AdviceKind::Before => "before",
                            crate::ast::AdviceKind::After => "after",
                            crate::ast::AdviceKind::Around => "around",
                        },
                        tok
                    ),
                    err_line,
                ));
            }
        };
        let body = self.parse_block()?;
        Ok(Statement {
            label: None,
            kind: StmtKind::AdviceDecl {
                kind,
                pattern,
                body,
            },
            line,
        })
    }

    /// `struct Name { field => Type, ... ; fn method { } }`
    fn parse_struct_decl(&mut self) -> StrykeResult<Statement> {
        let line = self.peek_line();
        self.advance(); // struct
        let raw_name = self.parse_package_qualified_identifier().map_err(|_| {
            self.syntax_err(
                format!("Expected struct name, got {:?}", self.peek()),
                self.peek_line(),
            )
        })?;
        let name = if raw_name.contains("::") || self.current_package == "main" {
            raw_name
        } else {
            format!("{}::{}", self.current_package, raw_name)
        };
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Check for method definition: `fn name { }` or `fn name { }`
            let is_method = match self.peek() {
                Token::Ident(s) => s == "fn" || s == "sub",
                _ => false,
            };
            if is_method {
                let is_sub_keyword = matches!(self.peek(), Token::Ident(ref s) if s == "sub");
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
                let body = if is_sub_keyword {
                    self.parse_block()?
                } else {
                    self.parse_fn_eq_body_or_block(false)?
                };
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
            // Support three forms:
            //   - `field => Type`   (Perl-style fat-comma)
            //   - `field: Type`     (Rust/class-style colon)
            //   - bare `field`      (implies Any type)
            let ty = if self.eat(&Token::FatArrow) || self.eat(&Token::Colon) {
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
    fn parse_enum_decl(&mut self) -> StrykeResult<Statement> {
        let line = self.peek_line();
        self.advance(); // enum
        let raw_name = self.parse_package_qualified_identifier().map_err(|_| {
            self.syntax_err(
                format!("Expected enum name, got {:?}", self.peek()),
                self.peek_line(),
            )
        })?;
        let name = if raw_name.contains("::") || self.current_package == "main" {
            raw_name
        } else {
            format!("{}::{}", self.current_package, raw_name)
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
    fn parse_class_decl(&mut self, is_abstract: bool, is_final: bool) -> StrykeResult<Statement> {
        use crate::ast::{ClassDef, ClassField, ClassMethod, ClassStaticField, Visibility};
        let line = self.peek_line();
        self.advance(); // class
        let raw_name = self.parse_package_qualified_identifier().map_err(|_| {
            self.syntax_err(
                format!("Expected class name, got {:?}", self.peek()),
                self.peek_line(),
            )
        })?;
        // Bare `class Point` inside `package Geo` registers as `Geo::Point`,
        // matching the unqualified-fn rule. Already-qualified names pass
        // through unchanged, and `main` keeps the bare spelling so
        // existing test code that calls `Point->new(...)` still resolves.
        let name = if raw_name.contains("::") || self.current_package == "main" {
            raw_name
        } else {
            format!("{}::{}", self.current_package, raw_name)
        };

        // Parse `extends Parent1, Parent2` (each may be namespaced: `Foo::Base`)
        let mut extends = Vec::new();
        if matches!(self.peek(), Token::Ident(ref s) if s == "extends") {
            self.advance(); // extends
            loop {
                let parent = self.parse_package_qualified_identifier().map_err(|_| {
                    self.syntax_err(
                        format!(
                            "Expected parent class name after `extends`, got {:?}",
                            self.peek()
                        ),
                        self.peek_line(),
                    )
                })?;
                extends.push(parent);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
        }

        // Parse `impl Trait1, Trait2` (each may be namespaced: `Foo::Trait`)
        let mut implements = Vec::new();
        if matches!(self.peek(), Token::Ident(ref s) if s == "impl") {
            self.advance(); // impl
            loop {
                let trait_name = self.parse_package_qualified_identifier().map_err(|_| {
                    self.syntax_err(
                        format!("Expected trait name after `impl`, got {:?}", self.peek()),
                        self.peek_line(),
                    )
                })?;
                implements.push(trait_name);
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

                // Body: `{ ... }`, or `= expr` (same rules as top-level `fn`), or omitted (abstract)
                let body = if let Some(b) = self.try_parse_fn_assign_shorthand_body()? {
                    Some(b)
                } else if matches!(self.peek(), Token::LBrace) {
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

            // Type via colon (`name: Type`) OR fat-comma (`name => Type`).
            // The Perl-flavored struct-style fat-comma is accepted on
            // classes for symmetry with struct fields.
            let ty = if self.eat(&Token::Colon) || self.eat(&Token::FatArrow) {
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
    fn parse_trait_decl(&mut self) -> StrykeResult<Statement> {
        use crate::ast::{ClassMethod, TraitDef, Visibility};
        let line = self.peek_line();
        self.advance(); // trait
        let raw_name = self.parse_package_qualified_identifier().map_err(|_| {
            self.syntax_err(
                format!("Expected trait name, got {:?}", self.peek()),
                self.peek_line(),
            )
        })?;
        let name = if raw_name.contains("::") || self.current_package == "main" {
            raw_name
        } else {
            format!("{}::{}", self.current_package, raw_name)
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

            // Body: `{ ... }`, `= expr`, or omitted (required method)
            let body = if let Some(b) = self.try_parse_fn_assign_shorthand_body()? {
                Some(b)
            } else if matches!(self.peek(), Token::LBrace) {
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
                list_context: false,
            }),
            ExprKind::ArrayVar(name) => Some(VarDecl {
                sigil: Sigil::Array,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
                list_context: false,
            }),
            ExprKind::HashVar(name) => Some(VarDecl {
                sigil: Sigil::Hash,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
                list_context: false,
            }),
            ExprKind::Typeglob(name) => Some(VarDecl {
                sigil: Sigil::Typeglob,
                name: name.clone(),
                initializer: None,
                frozen: false,
                type_annotation: None,
                list_context: false,
            }),
            _ => None,
        }
    }

    fn parse_decl_array_destructure(
        &mut self,
        keyword: &str,
        line: usize,
    ) -> StrykeResult<Statement> {
        self.expect(&Token::LBracket)?;
        let elems = self.parse_match_array_elems_until_rbracket()?;
        self.expect(&Token::Assign)?;
        self.suppress_scalar_hash_brace += 1;
        let rhs = self.parse_expression()?;
        self.suppress_scalar_hash_brace -= 1;
        let stmt = self.desugar_array_destructure(keyword, line, elems, rhs)?;
        self.parse_stmt_postfix_modifier(stmt)
    }

    fn parse_decl_hash_destructure(
        &mut self,
        keyword: &str,
        line: usize,
    ) -> StrykeResult<Statement> {
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
    ) -> StrykeResult<Statement> {
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
                list_context: false,
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
                            step: None,
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
                            list_context: false,
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
                            list_context: false,
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
    ) -> StrykeResult<Statement> {
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
                list_context: false,
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
                            list_context: false,
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
    ) -> StrykeResult<Statement> {
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
        let used_parens = self.eat(&Token::LParen);

        if used_parens {
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
        // my ($x) = @a  → list context on the scalar (gets first element, not count)
        if used_parens {
            for decl in &mut decls {
                decl.list_context = true;
            }
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
            let rhs_start_pos = self.pos;
            let mut val = self.parse_expression()?;
            let rhs_end_pos = self.pos;
            // Stryke implicit-coderef sugar: when the RHS contains a
            // *free* bare topic-slot reference (`_`, `_0`, `_1`, `_<`,
            // `_<<`, etc. — no `$` sigil), auto-wrap the RHS in
            // `fn { ... }`. Forces consistent coderef semantics across
            // every topic-using form:
            //   `my $sq  = _ * _`                     → CODE ref
            //   `my $up  = uc _`                      → CODE ref
            //   `my $rev = ~> _ >{...} rev join("")`  → CODE ref
            // To compute eagerly using the current topic, use the
            // explicit `$_` / `$_N` / `$_<` sigil-prefixed forms —
            // those keep Perl semantics and never auto-wrap.
            //
            // "Free" = at brace-depth 0 within the RHS token stream.
            // Any `_` inside `{ ... }` (closure body, hash literal,
            // map/grep/sort/match block) is bound to whatever defines
            // that block and doesn't trigger the wrap, so
            //   `my $r = call(fn { _ < 4 })`   — `_` is inner-fn's
            //   `my $h = { k => _ }`           — `_` is hash value at depth 1
            //   `my $kind = match { _ => x }`  — `_` is wildcard pattern
            // all stay eager.
            if !crate::compat_mode()
                && self.block_depth == 0
                && decls.len() == 1
                && matches!(decls[0].sigil, Sigil::Scalar)
                && !matches!(
                    val.kind,
                    ExprKind::CodeRef { .. }
                        | ExprKind::SubroutineRef(_)
                        | ExprKind::SubroutineCodeRef(_)
                        | ExprKind::DynamicSubCodeRef(_)
                )
                && self.rhs_has_free_bare_topic_slot(rhs_start_pos, rhs_end_pos)
            {
                let val_line = val.line;
                val = Expr {
                    kind: ExprKind::CodeRef {
                        params: Vec::new(),
                        body: vec![Statement {
                            label: None,
                            kind: StmtKind::Expression(val),
                            line: val_line,
                        }],
                    },
                    line: val_line,
                };
            }
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
                                "oursync" => StmtKind::OurSync(decls),
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
            "oursync" => StmtKind::OurSync(decls),
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

    fn parse_var_decl(&mut self, allow_type_annotation: bool) -> StrykeResult<VarDecl> {
        let mut decl = match self.advance() {
            (Token::ScalarVar(name), _) => VarDecl {
                sigil: Sigil::Scalar,
                name,
                initializer: None,
                frozen: false,
                type_annotation: None,
                list_context: false,
            },
            (Token::ArrayVar(name), _) => VarDecl {
                sigil: Sigil::Array,
                name,
                initializer: None,
                frozen: false,
                type_annotation: None,
                list_context: false,
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
                    list_context: false,
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
                    list_context: false,
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
                list_context: false,
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

    fn parse_type_name(&mut self) -> StrykeResult<PerlTypeName> {
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

    fn parse_package(&mut self) -> StrykeResult<Statement> {
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
        // Track the active package so subsequent `fn name(...)` decls can be
        // recognised as `Pkg::name` for shadow-of-builtin checks.
        self.current_package = full_name.clone();
        Ok(Statement {
            label: None,
            kind: StmtKind::Package { name: full_name },
            line,
        })
    }

    fn parse_use(&mut self) -> StrykeResult<Statement> {
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
                    let mut parse_overload_pairs = |this: &mut Self| -> StrykeResult<()> {
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
                // Imports must start on the SAME LINE as `use Module`.
                // Without this, a bare `use K8s` followed by `p "…"`
                // on the next line silently swallowed the `p` call as
                // an import expression — failing later with the
                // confusing "pragma import must be a compile-time
                // string" error pointing at the next-line statement.
                // The legitimate multi-line form uses `,` to continue.
                let on_same_line = self.peek_line() == tok_line;
                if on_same_line
                    && !matches!(self.peek(), Token::Semicolon | Token::Eof)
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

    fn parse_no(&mut self) -> StrykeResult<Statement> {
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

    fn parse_return(&mut self) -> StrykeResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'return'
                        // No-value return: terminator tokens AND any postfix statement-modifier
                        // keyword (`if`/`unless`/`while`/`until`/`for`/`foreach`). Without this
                        // the postfix-modifier check below never fires for valueless returns —
                        // `parse_assign_expr` would see `if` and look it up as a sub call,
                        // producing the misleading "Undefined subroutine &if" error.
        let val = if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof)
            || self.peek_is_postfix_stmt_modifier_keyword()
        {
            None
        } else {
            // Parse the operand as a comma-list — Perl's `return` is a
            // list-operator, so `return 1, 2, 3` returns the list (1, 2, 3).
            // (BUG-010) Stay below pipe-forward and stop at postfix
            // statement-modifier keywords like `if` / `unless`.
            let first = self.parse_assign_expr()?;
            if matches!(self.peek(), Token::Comma | Token::FatArrow) {
                let mut items = vec![first];
                while self.eat(&Token::Comma) || self.eat(&Token::FatArrow) {
                    if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof)
                        || self.peek_is_postfix_stmt_modifier_keyword()
                    {
                        break;
                    }
                    items.push(self.parse_assign_expr()?);
                }
                let line = items.first().map(|e| e.line).unwrap_or(line);
                Some(Expr {
                    kind: ExprKind::List(items),
                    line,
                })
            } else {
                Some(first)
            }
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

    fn parse_expression(&mut self) -> StrykeResult<Expr> {
        self.parse_comma_expr()
    }

    fn parse_comma_expr(&mut self) -> StrykeResult<Expr> {
        // Word-op precedence (or/and/not) sits ABOVE assignment in Perl —
        // `EXPR or $err = $@` parses as `EXPR or ($err = $@)`, NOT
        // `(EXPR or $err) = $@`. Entering through `parse_or_word` here
        // (instead of `parse_assign_expr` directly) gives `or`/`and`/`not`
        // looser binding than `=`, matching `perlop`. The deeper chain
        // (`parse_not_word → parse_assign_expr → parse_ternary → … →
        // parse_log_or → …`) handles tighter operators normally.
        let expr = self.parse_or_word()?;
        let mut exprs = vec![expr];
        while self.eat(&Token::Comma) || self.eat(&Token::FatArrow) {
            if matches!(
                self.peek(),
                Token::RParen | Token::RBracket | Token::RBrace | Token::Semicolon | Token::Eof
            ) {
                break; // trailing comma
            }
            exprs.push(self.parse_or_word()?);
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

    fn parse_assign_expr(&mut self) -> StrykeResult<Expr> {
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
            Token::XAssign => {
                // `$s x= N` has no matching `BinOp::Repeat`; desugar to
                // `$s = $s x N` so we can reuse the existing `ExprKind::Repeat`
                // evaluator (scalar-repeat path; list-repeat fires only when
                // the LHS is a syntactic list literal).
                self.advance();
                let r = self.parse_assign_expr()?;
                let lhs_for_repeat = expr.clone();
                Ok(Expr {
                    kind: ExprKind::Assign {
                        target: Box::new(expr),
                        value: Box::new(Expr {
                            kind: ExprKind::Repeat {
                                expr: Box::new(lhs_for_repeat),
                                count: Box::new(r),
                                list_repeat: false,
                            },
                            line,
                        }),
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

    fn parse_ternary(&mut self) -> StrykeResult<Expr> {
        let expr = self.parse_pipe_forward()?;
        if self.eat(&Token::Question) {
            let line = expr.line;
            self.suppress_colon_range = self.suppress_colon_range.saturating_add(1);
            let then_expr = self.parse_assign_expr();
            self.suppress_colon_range = self.suppress_colon_range.saturating_sub(1);
            let then_expr = then_expr?;
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
    fn parse_pipe_forward(&mut self) -> StrykeResult<Expr> {
        // After moving word-ops (or/and/not) above the assignment level,
        // pipe_forward must descend into `parse_range` (which itself
        // descends into `parse_log_or`) — calling `parse_or_word` here
        // would re-introduce `or` at a wrong place in the precedence chain
        // (it now sits above `parse_comma_expr`). We skip past `parse_range`
        // rather than `parse_log_or` so `..` stays reachable.
        let mut left = self.parse_range()?;
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
            // RHS of `|>` parses at the same precedence as the LHS — see
            // the comment at the top of `parse_pipe_forward` for why this
            // descends into `parse_range` instead of `parse_or_word`.
            let right_result = self.parse_range();
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
    fn pipe_forward_apply(&self, lhs: Expr, rhs: Expr, line: usize) -> StrykeResult<Expr> {
        let Expr { kind, line: rline } = rhs;
        let new_kind = match kind {
            // ── Generic / user-defined calls ───────────────────────────────────
            ExprKind::FuncCall { name, mut args } => {
                // Stryke builtins are unprefixed; `CORE::` callers route back to the
                // bare-name pipe-forward dispatch below.
                let dispatch_name: &str = name.strip_prefix("CORE::").unwrap_or(name.as_str());
                match dispatch_name {
                    "puniq" | "uniq" | "distinct" | "flatten" | "set" | "list_count"
                    | "list_size" | "count" | "size" | "cnt" | "len" | "with_index" | "shuffle"
                    | "shuffled" | "frequencies" | "freq" | "pfrequencies" | "pfreq"
                    | "interleave" | "ddump" | "stringify" | "str" | "lines" | "words"
                    | "chars" | "digits" | "letters" | "letters_uc" | "letters_lc"
                    | "punctuation" | "numbers" | "graphemes" | "columns" | "sentences"
                    | "paragraphs" | "sections" | "trim" | "avg" | "to_json" | "to_csv"
                    | "to_toml" | "to_yaml" | "to_xml" | "to_html" | "from_json" | "from_csv"
                    | "from_toml" | "from_yaml" | "from_xml" | "to_markdown" | "to_table"
                    | "xopen" | "clip" | "sparkline" | "bar_chart" | "flame" | "stddev"
                    | "squared" | "sq" | "square" | "cubed" | "cb" | "cube" | "normalize"
                    | "snake_case" | "camel_case" | "kebab_case" => {
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
                    "reduce" | "fold" => {
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
                    n if Self::is_block_then_list_pipe_builtin(n) => {
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
                    "take" | "head" | "tail" | "drop" => {
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
                        if self.thread_last_mode {
                            args.push(lhs);
                        } else {
                            args.insert(0, lhs);
                        }
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
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
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
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
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
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Print { handle, args }
            }
            ExprKind::Say { handle, mut args } => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Say { handle, args }
            }
            ExprKind::Printf { handle, mut args } => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Printf { handle, args }
            }
            ExprKind::Die(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Die(args)
            }
            ExprKind::Warn(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Warn(args)
            }

            // ── Sprintf: first-arg pipe threads lhs into the `format` slot ─────
            //   `"n=%d" |> sprintf(42)` → `sprintf("n=%d", 42)` is awkward,
            //   but piping the format string is the rarer case. Prepending
            //   to the values list gives `sprintf(format, lhs, ...args)` for
            //   the common `$n |> sprintf "count=%d"` case.
            ExprKind::Sprintf { format, mut args } => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Sprintf { format, args }
            }

            // ── System / exec / globbing / filesystem variadics ────────────────
            ExprKind::System(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::System(args)
            }
            ExprKind::Exec(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Exec(args)
            }
            ExprKind::Unlink(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Unlink(args)
            }
            ExprKind::Chmod(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Chmod(args)
            }
            ExprKind::Chown(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Chown(args)
            }
            ExprKind::Glob(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Glob(args)
            }
            ExprKind::Files(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Files(args)
            }
            ExprKind::Filesf(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Filesf(args)
            }
            ExprKind::FilesfRecursive(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::FilesfRecursive(args)
            }
            ExprKind::Dirs(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Dirs(args)
            }
            ExprKind::DirsRecursive(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::DirsRecursive(args)
            }
            ExprKind::SymLinks(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::SymLinks(args)
            }
            ExprKind::Sockets(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Sockets(args)
            }
            ExprKind::Pipes(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::Pipes(args)
            }
            ExprKind::BlockDevices(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::BlockDevices(args)
            }
            ExprKind::CharDevices(mut args) => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::CharDevices(args)
            }
            ExprKind::GlobPar { mut args, progress } => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
                ExprKind::GlobPar { args, progress }
            }
            ExprKind::ParSed { mut args, progress } => {
                if self.thread_last_mode {
                    args.push(lhs);
                } else {
                    args.insert(0, lhs);
                }
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
            ExprKind::Swallow(_) => ExprKind::Swallow(Box::new(lhs)),
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
            ExprKind::ParExpr { block, list: _ } => ExprKind::ParExpr {
                block,
                list: Box::new(lhs),
            },
            ExprKind::ParReduceExpr {
                extract_block,
                reduce_block,
                list: _,
            } => ExprKind::ParReduceExpr {
                extract_block,
                reduce_block,
                list: Box::new(lhs),
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
                    if crate::no_interop_mode() {
                        return Err(self.syntax_err(
                            "stryke uses `rev` instead of `reverse` (--no-interop)",
                            line,
                        ));
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
            // the inner coderef with `lhs` as `$_[0]`, matching `LHS |> fn { ... }`.
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
    fn parse_or_word(&mut self) -> StrykeResult<Expr> {
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

    fn parse_and_word(&mut self) -> StrykeResult<Expr> {
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

    fn parse_not_word(&mut self) -> StrykeResult<Expr> {
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
        // Descend into assignment level — `not` sits ABOVE `=` in Perl
        // precedence, so `not $x = 5` parses as `not ($x = 5)`.
        self.parse_assign_expr()
    }

    fn parse_log_or(&mut self) -> StrykeResult<Expr> {
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

    fn parse_log_and(&mut self) -> StrykeResult<Expr> {
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

    fn parse_bit_or(&mut self) -> StrykeResult<Expr> {
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

    fn parse_bit_xor(&mut self) -> StrykeResult<Expr> {
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

    fn parse_bit_and(&mut self) -> StrykeResult<Expr> {
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

    fn parse_equality(&mut self) -> StrykeResult<Expr> {
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

    fn parse_comparison(&mut self) -> StrykeResult<Expr> {
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

    fn parse_shift(&mut self) -> StrykeResult<Expr> {
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

    fn parse_addition(&mut self) -> StrykeResult<Expr> {
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

    fn parse_multiplication(&mut self) -> StrykeResult<Expr> {
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
                    // List-repeat fires when the LHS was just closed by a
                    // list-constructor paren (`(EXPR)`, `(LIST)`, `()`) or
                    // `qw(...)`. `parse_primary` records the post-close
                    // position; an exact match against `self.pos` here means
                    // no postfix consumed any tokens between the close and
                    // the `x`, so the LHS is intrinsically a list construct.
                    let list_repeat = self.list_construct_close_pos == Some(self.pos);
                    self.advance();
                    let right = self.parse_regex_bind()?;
                    left = Expr {
                        kind: ExprKind::Repeat {
                            expr: Box::new(left),
                            count: Box::new(right),
                            list_repeat,
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

    fn parse_regex_bind(&mut self) -> StrykeResult<Expr> {
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
    fn parse_thread_input(&mut self) -> StrykeResult<Expr> {
        self.suppress_slash_as_div = self.suppress_slash_as_div.saturating_add(1);
        let result = self.parse_range();
        self.suppress_slash_as_div = self.suppress_slash_as_div.saturating_sub(1);
        result
    }

    /// Parse `~p>` / `~p>>` parallel-chunk thread-macros. Equivalent to
    /// `par_reduce { stage1 |> stage2 |> ... } SOURCE`, with optional
    /// `||>` or `|then|` mid-pipeline boundary that switches to a normal
    /// `~>` / `~>>` continuation operating on the auto-merged result.
    fn parse_thread_macro_chunk_par(
        &mut self,
        line: usize,
        thread_last: bool,
    ) -> StrykeResult<Expr> {
        // Source: same parsing rules as `~>`.
        self.suppress_parenless_call = self.suppress_parenless_call.saturating_add(1);
        let source_expr = self.parse_thread_input();
        self.suppress_parenless_call = self.suppress_parenless_call.saturating_sub(1);
        let source_expr = source_expr?;

        // Per-chunk stage chain: stages operate on `@_` (the chunk elements)
        // which the par_reduce runtime binds as the argument array. Use
        // `pending_thread_input` to seed the stage chain with `@_`.
        self.pending_thread_input = Some(Expr {
            kind: ExprKind::ArrayVar("_".into()),
            line,
        });
        let chunk_chain = self.parse_thread_macro_inner(line, thread_last, None);
        self.pending_thread_input = None;
        let chunk_chain = chunk_chain?;

        // `parse_thread_macro_inner` (under pipe_rhs_depth > 0) wraps its
        // result as `fn { ... stages applied to $_[0] ... }`. Unwrap to
        // get the bare Block (`Vec<Statement>`) for the `par_reduce`
        // extract slot.
        let extract_block: Block = match chunk_chain.kind {
            ExprKind::CodeRef { params: _, body } => body,
            _ => vec![Statement {
                label: None,
                kind: StmtKind::Expression(chunk_chain),
                line,
            }],
        };

        let par_reduce = Expr {
            kind: ExprKind::ParReduceExpr {
                extract_block,
                reduce_block: None,
                list: Box::new(source_expr),
            },
            line,
        };

        // Check for `||>` / `|then|` boundary; if present, parse the
        // continuation as a normal `~>` / `~>>` thread macro with the
        // par_reduce result as its input.
        if self.eat_chunk_par_split_boundary() {
            return self.parse_thread_macro_continuation(par_reduce, line, thread_last);
        }
        Ok(par_reduce)
    }

    /// Parse `~d>` / `~d>>` distributed thread-macros. Same chunk-block
    /// semantics as `~p>` (stages operate on `@_`) but chunks ship to a
    /// `RemoteCluster` via the existing `cluster::run_cluster` dispatcher.
    /// Syntax: `~d> on EXPR SOURCE stage1 stage2 ...`. The `on EXPR` slot
    /// is required; without it the operator falls through to a syntax
    /// error (no implicit default-cluster in v1).
    fn parse_thread_macro_dist(&mut self, line: usize, thread_last: bool) -> StrykeResult<Expr> {
        // Required `on EXPR` — the cluster operand.
        let on_ok = matches!(self.peek(), Token::Ident(ref s) if s == "on");
        if !on_ok {
            return Err(
                self.syntax_err("~d>: expected `on <cluster-expr>` after the operator", line)
            );
        }
        self.advance(); // consume `on`
                        // Parse cluster expr — same parse-rules as a thread-macro input
                        // (avoid pulling stages into the cluster expression).
        self.suppress_parenless_call = self.suppress_parenless_call.saturating_add(1);
        // Without this, `on $cluster () map { … }` parses `()` as a postfix
        // indirect call on `$cluster`, stealing the empty list meant as SOURCE.
        // Zero-arg cluster from a scalar sub: `on ($factory())` or `on $f->()`.
        self.suppress_indirect_paren_call = self.suppress_indirect_paren_call.saturating_add(1);
        let cluster_expr = self.parse_thread_input();
        self.suppress_indirect_paren_call = self.suppress_indirect_paren_call.saturating_sub(1);
        self.suppress_parenless_call = self.suppress_parenless_call.saturating_sub(1);
        let cluster_expr = cluster_expr?;

        // Source list: same rules as `~p>` source.
        self.suppress_parenless_call = self.suppress_parenless_call.saturating_add(1);
        let source_expr = self.parse_thread_input();
        self.suppress_parenless_call = self.suppress_parenless_call.saturating_sub(1);
        let source_expr = source_expr?;

        // Stage chain seeded with `@_` — matches `~p>` chunk-block
        // semantics. The VM-side eval prepends `@_ = $_;` to the shipped
        // block source so the remote agent's `set_topic(chunk_flat_array)`
        // is reflected into `@_` before user stages run.
        self.pending_thread_input = Some(Expr {
            kind: ExprKind::ArrayVar("_".into()),
            line,
        });
        let chunk_chain = self.parse_thread_macro_inner(line, thread_last, None);
        self.pending_thread_input = None;
        let chunk_chain = chunk_chain?;

        let extract_block: Block = match chunk_chain.kind {
            ExprKind::CodeRef { params: _, body } => body,
            _ => vec![Statement {
                label: None,
                kind: StmtKind::Expression(chunk_chain),
                line,
            }],
        };

        let dist_reduce = Expr {
            kind: ExprKind::DistReduceExpr {
                cluster: Box::new(cluster_expr),
                extract_block,
                list: Box::new(source_expr),
            },
            line,
        };

        // `||>` / `|then|` boundary continuation, same as `~p>`.
        if self.eat_chunk_par_split_boundary() {
            return self.parse_thread_macro_continuation(dist_reduce, line, thread_last);
        }
        Ok(dist_reduce)
    }

    /// Parse a `~>` / `~>>` continuation after a `||>` / `|then|`
    /// chunk-parallel-to-sequential boundary. Reuses
    /// `parse_thread_macro_inner` with `result_init: Some(prior)` so the
    /// stage loop threads from the par_reduce result instead of parsing
    /// a fresh source expression.
    fn parse_thread_macro_continuation(
        &mut self,
        prior: Expr,
        line: usize,
        thread_last: bool,
    ) -> StrykeResult<Expr> {
        self.pending_thread_input = Some(prior);
        let res = self.parse_thread_macro_inner(line, thread_last, None);
        self.pending_thread_input = None;
        res
    }

    /// Try to consume `||>` (LogOr followed by `>`) or `|then|`
    /// (`Pipe Ident("then") Pipe`) as the chunk-parallel → sequential
    /// switch marker. Returns true if a boundary was consumed.
    fn eat_chunk_par_split_boundary(&mut self) -> bool {
        // `||>` = `LogOr` token (already merged in lex) followed by `>`.
        if matches!(self.peek(), Token::LogOr) && matches!(self.peek_at(1), Token::NumGt) {
            self.advance(); // ||
            self.advance(); // >
            return true;
        }
        // `|then|` = `BitOr` + `Ident("then")` + `BitOr`.
        if matches!(self.peek(), Token::BitOr) {
            if let Token::Ident(name) = self.peek_at(1).clone() {
                if name == "then" && matches!(self.peek_at(2), Token::BitOr) {
                    self.advance(); // |
                    self.advance(); // then
                    self.advance(); // |
                    return true;
                }
            }
        }
        false
    }

    /// Perl `..` / `...` operator — precedence sits between `?:` and `||` (`perlop`), so
    /// `$x .. $x + 3` parses as `$x .. ($x + 3)` and `1..$n||5` parses as `1..($n||5)`. Both
    /// operands recurse through `parse_log_or`, which in turn walks down through all tighter
    /// operators (additive, multiplicative, regex bind, unary). Non-associative: the right
    /// operand is a single `parse_log_or` so `1..5..10` is a parse error in Perl, but we accept
    /// it greedily (left-associated) because the lexer already forbids `..` after a range RHS.
    fn parse_range(&mut self) -> StrykeResult<Expr> {
        let left = self.parse_log_or()?;
        let line = left.line;
        // `1..10` (traditional inclusive) / `1...10` (exclusive) / `1:10`
        // (short form) / `1~10` (universal short form). The `~` separator
        // works for every range type and is the only viable separator for
        // IPv6 since IPv6 already uses `:` internally; `:` would collide.
        // It also dodges `!`'s collision with the `_!N!` paired char-index
        // syntax. Single-`~` (vs `!!!` triple) keeps the surface simple.
        let (exclusive, _colon_style) = if self.eat(&Token::RangeExclusive) {
            (true, false)
        } else if self.eat(&Token::Range) {
            (false, false)
        } else if self.suppress_colon_range == 0 && self.eat(&Token::Colon) {
            // `1:10` short form — only valid for numeric ranges, not ternary
            // Lookahead: must be followed by something that looks like a range endpoint
            (false, true)
        } else if self.suppress_tilde_range == 0 && self.eat(&Token::BitNot) {
            (false, true)
        } else {
            return Ok(left);
        };
        let right = self.parse_log_or()?;
        // Optional step: `1..100:2` / `1:100:2` / `IPV6~IPV6~STEP`. `~` is
        // gated by `suppress_tilde_range` so paired char-index (`$x~5~`)
        // doesn't get its closing delimiter eaten as a range op.
        let step = if self.eat(&Token::Colon)
            || (self.suppress_tilde_range == 0 && self.eat(&Token::BitNot))
        {
            Some(Box::new(self.parse_unary()?))
        } else {
            None
        };
        Ok(Expr {
            kind: ExprKind::Range {
                from: Box::new(left),
                to: Box::new(right),
                exclusive,
                step,
            },
            line,
        })
    }

    /// `name` or `Foo::Bar::baz` — used after `sub`, unary `&`, etc.
    fn parse_package_qualified_identifier(&mut self) -> StrykeResult<String> {
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
                // Topic-slot scalars (`_`, `_<<<<`, `_3`, etc.) lex as
                // `Token::ScalarVar` per the lexer's reservation. Accept
                // them as the trailing segment of a package-qualified
                // name so callers (e.g. `parse_sub_decl`) can reject the
                // full name with a friendly "would shadow topic-slot"
                // message rather than a generic "Expected identifier
                // after `::`" lexer-level error.
                (Token::ScalarVar(part), _) if Self::is_underscore_topic_slot(&part) => {
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
    fn parse_qualified_subroutine_name(&mut self) -> StrykeResult<String> {
        self.parse_package_qualified_identifier()
    }

    fn parse_unary(&mut self) -> StrykeResult<Expr> {
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
            // Special case: `+{ ... }` forces hashref interpretation (Perl idiom),
            // even when the body is a list-yielding expression like `+{ map { ... } @arr }`.
            // Without this, `{ map { ... } @arr }` falls back to block/CodeRef parsing
            // because the body doesn't fit `KEY => VAL` shape.
            Token::Plus => {
                self.advance();
                if matches!(self.peek(), Token::LBrace) {
                    let line = self.peek_line();
                    self.advance(); // consume {
                    return self.parse_forced_hashref_body(line);
                }
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

    fn parse_power(&mut self) -> StrykeResult<Expr> {
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

    fn parse_postfix(&mut self) -> StrykeResult<Expr> {
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
                                    let indices = self.parse_slice_arg_list(false)?;
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
                                    let keys = self.parse_slice_arg_list(true)?;
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
                    // Implicit semicolon: `[` on a new line is a new statement (array literal),
                    // not an array subscript on the preceding expression.
                    if self.peek_line() > self.prev_line() {
                        break;
                    }
                    // `$a[i]` — or chained `$r->{k}[i]` / `$a[1][2]` — or list slice `(sort ...)[0]`.
                    let line = expr.line;
                    if matches!(expr.kind, ExprKind::ScalarVar(_)) {
                        if let ExprKind::ScalarVar(ref name) = expr.kind {
                            let name = name.clone();
                            self.advance();
                            // Parse full expression to handle comma operator correctly:
                            // `$a[1, 2]` evaluates comma expr (returns last value = 2)
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
                Token::LogNot | Token::BitNot => {
                    // Stryke universal string-subscript sugar — paired `!…!`
                    // OR paired `~…~`: `$VAR!N!`, `$VAR~N~`, `$VAR!1:5:2!`,
                    // `_!N!`, `_~from:to:step~`. Returns substring of the
                    // scalar (Unicode chars).  Distinct from `[N]` which has
                    // Perl's `@VAR[N]` / `$_[N]` semantics. Both forms work on
                    // any scalar (named or topic) without colliding: `!` and
                    // `~` after a value have no current postfix meaning (`!=`
                    // / `!~` are pre-merged binary tokens; `~` is prefix-only
                    // bit-not). The opening and closing delimiter must match.
                    //
                    // Implementation: rewrite to ArrayElement with a
                    // synthetic name `__topicstr__$NAME`. The interpreter
                    // and VM strip the prefix and dispatch to char-of-string
                    // (and slice-of-string for Range indices).
                    if !matches!(expr.kind, ExprKind::ScalarVar(_)) {
                        break;
                    }
                    if self.peek_line() > self.prev_line() {
                        break;
                    }
                    let opener = self.peek().clone();
                    let line = expr.line;
                    let name = if let ExprKind::ScalarVar(ref n) = expr.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    self.advance(); // consume opening `!` or `~`
                                    // Suppress `~` as a range separator while parsing the
                                    // paired index — `$_~5~` would otherwise consume the
                                    // closing `~` as a range op. `:` is still allowed so
                                    // `$_~1:3~` (slice with `:` range index) keeps working.
                    self.suppress_tilde_range = self.suppress_tilde_range.saturating_add(1);
                    let index_result = self.parse_expression();
                    self.suppress_tilde_range = self.suppress_tilde_range.saturating_sub(1);
                    let index = index_result?;
                    let close_match = matches!(
                        (&opener, self.peek()),
                        (Token::LogNot, Token::LogNot) | (Token::BitNot, Token::BitNot)
                    );
                    if !close_match {
                        let want = if matches!(opener, Token::LogNot) {
                            "!"
                        } else {
                            "~"
                        };
                        return Err(self.syntax_err(
                            format!("expected closing `{}` for string subscript", want),
                            self.peek_line(),
                        ));
                    }
                    self.advance(); // consume closing delimiter
                    expr = Expr {
                        kind: ExprKind::ArrayElement {
                            array: format!("__topicstr__{}", name),
                            index: Box::new(index),
                        },
                        line,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> StrykeResult<Expr> {
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
                // `qw(a b c) x N` is list-repeat in Perl even without explicit
                // outer parens — `qw(...)` is itself a list constructor.
                self.list_construct_close_pos = Some(self.pos);
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
                        let indices = self.parse_slice_arg_list(false)?;
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
                        let keys = self.parse_slice_arg_list(true)?;
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
                // `%h{KEYS}` — Perl 5.20+ key-value slice. Parser-level
                // disambiguation: `%h` immediately followed by `{` is a kv-
                // slice; `%h` alone (or followed by `=`, list ops, etc.) is
                // the bare hash. (BUG-008)
                if matches!(self.peek(), Token::LBrace) && self.suppress_scalar_hash_brace == 0 {
                    self.advance(); // {
                    let keys = self.parse_slice_arg_list(true)?;
                    self.expect(&Token::RBrace)?;
                    return Ok(Expr {
                        kind: ExprKind::HashKvSlice { hash: name, keys },
                        line,
                    });
                }
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
                    // `@{$href}{k1,k2}` — hash slice through a hashref using
                    // the curly-brace deref form. Mirrors the `@$href{KEYS}`
                    // path (BUG-091/BUG-217). Likewise `@{$aref}[i,j]` is the
                    // array-slice-through-arrayref form.
                    if matches!(self.peek(), Token::LBrace) {
                        self.advance();
                        let keys = self.parse_slice_arg_list(true)?;
                        self.expect(&Token::RBrace)?;
                        return Ok(Expr {
                            kind: ExprKind::HashSliceDeref {
                                container: Box::new(inner),
                                keys,
                            },
                            line,
                        });
                    }
                    if matches!(self.peek(), Token::LBracket) {
                        self.advance();
                        let indices = self.parse_slice_arg_list(false)?;
                        self.expect(&Token::RBracket)?;
                        let source = Expr {
                            kind: ExprKind::Deref {
                                expr: Box::new(inner),
                                kind: Sigil::Array,
                            },
                            line,
                        };
                        return Ok(Expr {
                            kind: ExprKind::AnonymousListSlice {
                                source: Box::new(source),
                                indices,
                            },
                            line,
                        });
                    }
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
                    let keys = self.parse_slice_arg_list(true)?;
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
                    // Empty `() x 3` is a no-op list repeat — record the close
                    // position so `Token::X` knows the LHS was a list literal.
                    self.list_construct_close_pos = Some(self.pos);
                    return Ok(Expr {
                        kind: ExprKind::List(vec![]),
                        line,
                    });
                }
                // Inside parens, pipe-forward is allowed even if we're in a
                // paren-less arg context. Save and restore no_pipe_forward_depth.
                let saved_no_pipe = self.no_pipe_forward_depth;
                self.no_pipe_forward_depth = 0;
                // Thread-macro `on` may set `suppress_indirect_paren_call` so
                // `on $c ()` does not steal `()`; inside explicit `(...)` use
                // normal postfix-`(` rules (`on ($factory())`).
                let saved_indirect = self.suppress_indirect_paren_call;
                self.suppress_indirect_paren_call = 0;
                let expr = self.parse_expression();
                self.no_pipe_forward_depth = saved_no_pipe;
                self.suppress_indirect_paren_call = saved_indirect;
                let expr = expr?;
                self.expect(&Token::RParen)?;
                // Mark this paren as a list-constructor for the `x` operator
                // (parse_multiplication compares `self.pos` at the X token to
                // this checkpoint). Function-call parens (`f(args)`) don't
                // reach this branch; they're parsed by the call machinery.
                self.list_construct_close_pos = Some(self.pos);
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
                self.parse_thread_macro(line, false)
            }
            Token::ThreadArrowLast => {
                self.advance();
                self.parse_thread_macro(line, true)
            }
            Token::ThreadArrowStream => {
                self.advance();
                let mut stages = Vec::new();
                self.parse_thread_macro_inner(line, false, Some(&mut stages))
            }
            Token::ThreadArrowStreamLast => {
                self.advance();
                let mut stages = Vec::new();
                self.parse_thread_macro_inner(line, true, Some(&mut stages))
            }
            Token::ThreadArrowPar => {
                self.advance();
                self.parse_thread_macro_chunk_par(line, false)
            }
            Token::ThreadArrowParLast => {
                self.advance();
                self.parse_thread_macro_chunk_par(line, true)
            }
            Token::ThreadArrowDist => {
                self.advance();
                self.parse_thread_macro_dist(line, false)
            }
            Token::ThreadArrowDistLast => {
                self.advance();
                self.parse_thread_macro_dist(line, true)
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

    fn parse_named_expr(&mut self, mut name: String) -> StrykeResult<Expr> {
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
        // Stryke exception: topic-slot barewords (`_`, `_<`, `_0`, `_0<`, …) are
        // scalar references to the topic / positional / outer-topic chain — they
        // must evaluate as the topic value, not the literal name.
        if matches!(self.peek(), Token::FatArrow) && !Self::is_underscore_topic_slot(&name) {
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

        // `CORE::length(...)` etc. — strip the explicit core-dispatch prefix so
        // the keyword arms below match the bare name and produce the same
        // `ExprKind::Length` / `ExprKind::Print` / etc. as the unprefixed form.
        // Matches Perl 5's `CORE::` namespace, which routes back to the
        // built-in implementation regardless of any same-named user sub.
        // (PARITY-011)
        if let Some(rest) = name.strip_prefix("CORE::") {
            name = rest.to_string();
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
            // `__PACKAGE__` is a compile-time constant set to the currently
            // active package, so a sub body in `package Demo::P1` keeps
            // returning `"Demo::P1"` regardless of the caller's package
            // (Perl 5 documented behavior).
            "__PACKAGE__" => Ok(Expr {
                kind: ExprKind::String(self.current_package.clone()),
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
                if crate::no_interop_mode() {
                    return Err(
                        self.syntax_err("stryke uses `p` instead of `say` (--no-interop)", line)
                    );
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
                // Named-unary precedence: `defined X && Y` is `(defined X) && Y`,
                // not `defined(X && Y)`. The default `parse_one_arg_or_default`
                // path is greedy (calls `parse_assign_expr_stop_at_pipe`), which
                // would let `&&` bind into the argument and silently make
                // `defined $h{k} && $h{k} > 0`-style guards always-true when the
                // hash element existed. `parse_named_unary_arg` stops at shift
                // level so logical operators stay outside.
                let a = if matches!(
                    self.peek(),
                    Token::Semicolon
                        | Token::RBrace
                        | Token::RParen
                        | Token::RBracket
                        | Token::Eof
                        | Token::Comma
                        | Token::FatArrow
                        | Token::PipeForward
                        | Token::Question
                        | Token::Colon
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
                        | Token::LogNot
                        | Token::LogAndWord
                        | Token::LogOrWord
                        | Token::LogNotWord
                        | Token::DefinedOr
                        | Token::Range
                        | Token::RangeExclusive
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
                ) {
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: self.peek_line(),
                    }
                } else if matches!(self.peek(), Token::LParen)
                    && matches!(self.peek_at(1), Token::RParen)
                {
                    let pl = self.peek_line();
                    self.advance();
                    self.advance();
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: pl,
                    }
                } else {
                    self.parse_named_unary_arg()?
                };
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
                if crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke uses `len` (also `cnt` / `count`) instead of `scalar` (--no-interop)",
                        line,
                    ));
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
                // Perl 5.24+ rejects `push SCALAR, ...` at parse time. Reject any
                // first arg that is unambiguously a scalar (literal scalar var or
                // numeric/string literal). Array refs (`@$x`), bindings, slices,
                // and `our @a` style remain permitted.
                if matches!(
                    first.kind,
                    ExprKind::ScalarVar(_)
                        | ExprKind::Integer(_)
                        | ExprKind::Float(_)
                        | ExprKind::String(_)
                ) {
                    return Err(self
                        .syntax_err("Experimental push on scalar is now forbidden", line)
                        .with_near("at EOF"));
                }
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
            // `splice_last(@a, off[, n])` is the stryke spelling of Perl's
            // `scalar splice(@a, off, n)` — returns the LAST removed element
            // (or undef if nothing was removed). Desugars to `tail(splice(...))`
            // so the array is still mutated in place.
            "splice_last" | "splice1" | "spl_last" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                let mut iter = args.into_iter();
                let array = Box::new(
                    iter.next()
                        .ok_or_else(|| self.syntax_err("splice_last requires arguments", line))?,
                );
                let offset = iter.next().map(Box::new);
                let length = iter.next().map(Box::new);
                let replacement: Vec<Expr> = iter.collect();
                let splice_expr = Expr {
                    kind: ExprKind::Splice {
                        array,
                        offset,
                        length,
                        replacement,
                    },
                    line,
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: "tail".to_string(),
                        args: vec![splice_expr],
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
                // `parse_postfix` starts at `parse_primary` which doesn't
                // accept the leading `&` of `&subname` — call `parse_unary`
                // instead so `exists &main::myf` parses the same as
                // `defined &main::myf` already does.
                let a = self.parse_unary()?;
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
                    // Two surface forms share this branch:
                    //   `fore EXPR, LIST` — comma form (explicit per-item EXPR + list)
                    //   `ep LIST`         — list-only form: print each item with `say $_`
                    // We disambiguate by peeking after the first parsed expression:
                    // if the next token is a comma we're in the EXPR-then-LIST form;
                    // otherwise the first parse *was* the LIST and we default the
                    // block to `say $_` (only for `ep` — `fore`/`e` keep their
                    // explicit-expression contract).
                    let expr = self.parse_assign_expr()?;
                    let expr = Self::lift_bareword_to_topic_call(expr);
                    if !matches!(self.peek(), Token::Comma) && name == "ep" {
                        let block = vec![Statement {
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
                        }];
                        return Ok(Expr {
                            kind: ExprKind::ForEachExpr {
                                block,
                                list: Box::new(expr),
                            },
                            line,
                        });
                    }
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
                // List-operator precedence (so `rev 1..3` parses as `rev(1..3)`, not
                // `(rev 1)..3`). Defaults to $_ when no argument given.
                // Only use pipe placeholder when directly in pipe RHS (not inside a block).
                // RBrace means we're inside a block like `map { rev }` - use $_ default.
                let prev = self.prev_line();
                let a = if self.in_pipe_rhs()
                    && (matches!(
                        self.peek(),
                        Token::Semicolon | Token::RParen | Token::Eof | Token::PipeForward
                    ) || self.peek_line() > prev)
                {
                    self.pipe_placeholder_list(line)
                } else if self.peek_line() > prev {
                    // Newline boundary: argument is on a later line —
                    // default to `$_` so the next statement parses as
                    // its own thing instead of being slurped as the
                    // implicit operand. (Same rule as
                    // `parse_one_arg_or_default`.)
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: prev,
                    }
                } else if matches!(
                    self.peek(),
                    Token::Semicolon
                        | Token::RBrace
                        | Token::RParen
                        | Token::RBracket
                        | Token::Eof
                        | Token::Comma
                        | Token::FatArrow
                        | Token::PipeForward
                ) {
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: self.peek_line(),
                    }
                } else if matches!(self.peek(), Token::LParen)
                    && matches!(self.peek_at(1), Token::RParen)
                {
                    // `rev()` — empty parens default to `$_` (matches Perl's
                    // `length()` / `uc()` etc. and the `|> rev()` pipe form).
                    let pl = self.peek_line();
                    self.advance(); // (
                    self.advance(); // )
                    Expr {
                        kind: ExprKind::ScalarVar("_".into()),
                        line: pl,
                    }
                } else {
                    self.parse_one_arg()?
                };
                Ok(Expr {
                    kind: ExprKind::Rev(Box::new(a)),
                    line,
                })
            }
            "reverse" => {
                if crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke uses `rev` instead of `reverse` (--no-interop)",
                        line,
                    ));
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
                } else if matches!(self.peek(), Token::LParen)
                    && matches!(self.peek_at(1), Token::RParen)
                {
                    // `reverse()` — Perl-style empty list call returns the empty list.
                    self.advance();
                    self.advance();
                    Expr {
                        kind: ExprKind::List(Vec::new()),
                        line,
                    }
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
                    let list_expr = if self.pipe_supplies_slurped_list_operand() {
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
            "cond" => {
                if crate::compat_mode() {
                    return Err(self
                        .syntax_err("`cond` is a stryke extension (disabled by --compat)", line));
                }
                self.parse_cond_expr(line)
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
                    if self.pipe_supplies_slurped_list_operand() {
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
                                // Non-literal (e.g. `defined`, scalar coderef var,
                                // hash slot): lift barewords to topic-call, then
                                // route through GrepExprComma so the runtime
                                // coderef-dispatch in Op::GrepWithExpr handles
                                // both truthiness AND coderef-call uniformly.
                                let expr = Self::lift_bareword_to_topic_call(expr);
                                return Ok(Expr {
                                    kind: ExprKind::GrepExprComma {
                                        expr: Box::new(expr),
                                        list: Box::new(list),
                                        keyword,
                                    },
                                    line,
                                });
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
                    let block_end_line = self.prev_line();
                    let _ = self.eat(&Token::Comma);
                    let list = if self.in_pipe_rhs()
                        && (matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) || self.peek_line() > block_end_line)
                    {
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
                    // `sort $coderef (LIST)` — comparator is first; list often parenthesized.
                    // Pipe-RHS form `|> sort $coderef` uses placeholder LHS as the list.
                    self.suppress_indirect_paren_call =
                        self.suppress_indirect_paren_call.saturating_add(1);
                    let code = self.parse_assign_expr()?;
                    self.suppress_indirect_paren_call =
                        self.suppress_indirect_paren_call.saturating_sub(1);
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
                    } else if matches!(self.peek(), Token::LParen) {
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
                    // Treat a newline as an implicit pipeline terminator —
                    // `@a |> sort\nmy $x = ...` must NOT swallow the next
                    // `my` stmt as sort's argument list.
                    let list = if self.in_pipe_rhs()
                        && (matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) || self.peek_line() > line)
                    {
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
                if matches!(self.peek(), Token::LParen) {
                    self.expect(&Token::LParen)?;
                    let list = self.parse_expression()?;
                    self.expect(&Token::RParen)?;
                    let block = self.parse_block()?;
                    Ok(Expr {
                        kind: ExprKind::PForExpr {
                            block,
                            list: Box::new(list),
                            progress: None,
                        },
                        line,
                    })
                } else {
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
            }
            // `par { BLOCK } LIST` — generic parallel-chunk evaluator.
            // Splits LIST into chunks (UTF-8-aligned for strings,
            // element-aligned for arrays), runs BLOCK on each chunk in
            // parallel with `_` bound to the chunk, flattens results.
            // Available as a top-level expression, not just an `~>` stage.
            "par" => {
                let (block, list, _progress) = self.parse_block_then_list_optional_progress()?;
                Ok(Expr {
                    kind: ExprKind::ParExpr {
                        block,
                        list: Box::new(list),
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
                // `thread EXPR stage1 stage2 ...` — threading macro (thread-first)
                // `t` is a short alias for `thread`
                // Each stage is either:
                //   - `ident` — bare function call
                //   - `ident { block }` — function with block arg
                //   - `ident arg1 arg2 { block }` — function with args and optional block
                //   - `fn { block }` — standalone anonymous block
                //   - `>{ block }` — shorthand for standalone anonymous block
                // Desugars to: EXPR |> stage1 |> stage2 |> ...
                self.parse_thread_macro(line, false)
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
            "swallow" | "swa" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let a = self.parse_one_arg_or_default()?;
                Ok(Expr {
                    kind: ExprKind::Swallow(Box::new(a)),
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
                    // Mirror `sort`'s pipe-RHS handling — after the block,
                    // a newline (or any standard terminator token) inside a
                    // `|> psort { ... }` chain means the list comes from the
                    // pipe LHS, not from continued parsing into the next
                    // statement. Without this check `(@list) |> psort {
                    // _0 <=> _1 }\nmy $n = ...` silently swallowed `my $n =
                    // ...` as the list operand.
                    let block_end_line = self.prev_line();
                    self.eat(&Token::Comma);
                    let use_placeholder = self.in_pipe_rhs()
                        && (matches!(
                            self.peek(),
                            Token::Semicolon
                                | Token::RBrace
                                | Token::RParen
                                | Token::Eof
                                | Token::PipeForward
                        ) || self.peek_line() > block_end_line);
                    let (list, progress) = if use_placeholder {
                        (self.pipe_placeholder_list(line), None)
                    } else {
                        self.parse_assign_expr_list_optional_progress()?
                    };
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
                // `len(EXPR)` / `cnt(EXPR)` / `count(EXPR)` with a tight `(` —
                // the parens are function-call syntax, not a parenthesized
                // list: stop the argument at `)` so `len(@a) % 2 == 1` is
                // `(len(@a)) % 2 == 1`, not `len(@a % 2 == 1)`. Empty parens
                // `len()` collapse to a zero-arg call (use the piped operand
                // or `$_`). Bare `len` followed by a low-precedence operator
                // (`==`, `&&`, `?`, …) also defaults to a zero-arg call so
                // `{ len == 0 }` works as a block predicate on the topic.
                // Bare `len EXPR` (no parens, e.g. `len @arr`) goes through
                // the greedy list-arg parser; this means `len @a + len @b`
                // is `len(@a + len(@b))` (returning the length of the sum
                // string), not `(len @a) + (len @b)`. Use explicit parens
                // when combining `len` with `+`, `-`, comparisons, etc.
                let args = if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Vec::new()
                    } else {
                        let inner = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        vec![inner]
                    }
                } else if self.peek_is_named_unary_terminator() {
                    Vec::new()
                } else {
                    let (list, progress) = self.parse_assign_expr_list_optional_progress()?;
                    if progress.is_some() {
                        return Err(self.syntax_err(
                            "`progress =>` is not supported for list_count / list_size / count / cnt",
                            line,
                        ));
                    }
                    vec![list]
                };
                Ok(Expr {
                    kind: ExprKind::FuncCall {
                        name: name.clone(),
                        args,
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
                // `any(CODEREF, LIST)` with parens — parse as normal call.
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.clone(),
                            args,
                        },
                        line,
                    });
                }
                // Coderef-in-block-position: `any $f LIST` / `any $f, LIST` /
                // `LIST |> any $f`. Same shape as the block form but uses a
                // value expression where `{ BLOCK }` would go.
                if let Some(args) = self.try_parse_coderef_listop_args(line)? {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.clone(),
                            args,
                        },
                        line,
                    });
                }
                // `any BLOCK LIST` without parens.
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
            // Ruby `detect` / `find` — same as `first` (first element matching block).
            "first" | "detect" | "find" | "find_index" | "firstidx" | "first_index" => {
                let canonical =
                    if matches!(name.as_str(), "find_index" | "firstidx" | "first_index") {
                        "find_index"
                    } else {
                        "first"
                    };
                // `first(CODEREF, LIST)` with parens — parse as normal call.
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: canonical.to_string(),
                            args,
                        },
                        line,
                    });
                }
                // Coderef-in-block-position: `first $f LIST` / `LIST |> first $f`.
                if let Some(args) = self.try_parse_coderef_listop_args(line)? {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: canonical.to_string(),
                            args,
                        },
                        line,
                    });
                }
                // `first BLOCK LIST` without parens.
                let (block, list, progress) = self.parse_block_then_list_optional_progress()?;
                if progress.is_some() {
                    return Err(self.syntax_err(
                        "`progress =>` is not supported for first/detect/find/find_index (use pfirst for parallel + progress)",
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
                        name: canonical.to_string(),
                        args: vec![cr, list],
                    },
                    line,
                })
            }
            "take_while" | "drop_while" | "skip_while" | "reject" | "grepv" | "tap" | "peek"
            | "partition" | "min_by" | "max_by" | "zip_with" | "count_by" => {
                // Coderef-in-block-position: `take_while $f LIST` etc.
                if let Some(args) = self.try_parse_coderef_listop_args(line)? {
                    return Ok(Expr {
                        kind: ExprKind::FuncCall {
                            name: name.to_string(),
                            args,
                        },
                        line,
                    });
                }
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
                    // Perl convention: `open FH, "<", path` — an all-uppercase
                    // (or `_`) bareword in the filehandle slot is a literal
                    // handle name, never a constant / sub / bareword
                    // expression. Without this special-case, registered
                    // constants like `PI` / `TAU` / `E` would override the
                    // documented Perl idiom and the handle would register
                    // under the constant's numeric value.
                    let handle_lit = self.take_bareword_filehandle();
                    if handle_lit.is_some() {
                        // Consume the comma after the bareword filehandle so
                        // the arg parser starts at the mode expression.
                        self.expect(&Token::Comma)?;
                    }
                    let args = if paren {
                        self.parse_arg_list()?
                    } else {
                        self.parse_list_until_terminator()?
                    };
                    if paren {
                        self.expect(&Token::RParen)?;
                    }
                    let total = handle_lit.is_some() as usize + args.len();
                    if total < 2 {
                        return Err(self.syntax_err("open requires at least 2 arguments", line));
                    }
                    let (handle_expr, mode_expr, file_expr) = match handle_lit {
                        Some(name) => {
                            let h = Expr {
                                kind: ExprKind::String(name),
                                line,
                            };
                            (h, args[0].clone(), args.get(1).cloned())
                        }
                        None => (args[0].clone(), args[1].clone(), args.get(2).cloned()),
                    };
                    Ok(Expr {
                        kind: ExprKind::Open {
                            handle: Box::new(handle_expr),
                            mode: Box::new(mode_expr),
                            file: file_expr.map(Box::new),
                        },
                        line,
                    })
                }
            }
            "close" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                // `close FH` — bareword filehandle slot takes a literal name.
                let a = self
                    .take_bareword_filehandle_arg(line)
                    .map(Ok)
                    .unwrap_or_else(|| self.parse_one_arg_or_default())?;
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
                // `eof FH` — bareword filehandle slot (no parens) takes a
                // literal name. `eof(FH)` / `eof($fh)` / `eof("FH")` keep
                // their general-expression handling.
                if let Some(a) = self.take_bareword_filehandle_arg(line) {
                    return Ok(Expr {
                        kind: ExprKind::Eof(Some(Box::new(a))),
                        line,
                    });
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
                        // Inside the parens, bareword still wins as a handle.
                        let a = self
                            .take_bareword_filehandle_arg(line)
                            .map(Ok)
                            .unwrap_or_else(|| self.parse_expression())?;
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
            "exe" | "executables" => {
                if let Some(e) = self.fat_arrow_autoquote(&name, line) {
                    return Ok(e);
                }
                let args = self.parse_builtin_args()?;
                Ok(Expr {
                    kind: ExprKind::Executables(args),
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
                if crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke `wantarray` is rejected under --no-interop — \
                         use explicit return-shape (`@result` vs `$scalar`) \
                         or pass a flag arg instead of context-sniffing",
                        line,
                    ));
                }
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
                // In no-interop mode, `sub {}` is not valid — must use `fn {}`
                if crate::no_interop_mode() {
                    return Err(self.syntax_err(
                        "stryke uses `fn {}` instead of `sub {}` (--no-interop)",
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
                self.parse_sub_attributes()?;
                let body = self.parse_fn_eq_body_or_block(false)?;
                Ok(Expr {
                    kind: ExprKind::CodeRef { params, body },
                    line,
                })
            }
            _ => {
                // Generic function call
                // Check for fat arrow (bareword string in hash) — except for
                // topic-slot barewords (`_`, `_<`, `_0`, `_0<`, …), which must
                // resolve to the topic value, not the literal name.
                if matches!(self.peek(), Token::FatArrow) && !Self::is_underscore_topic_slot(&name)
                {
                    return Ok(Expr {
                        kind: ExprKind::String(name),
                        line,
                    });
                }
                // Bare `_` in expression position → topic variable `$_`.
                // Allows concise blocks: `map { _ * 2 }`, `fi { _ > 5 }`.
                // Also handles the outer-topic chain: `_<`, `_<<`, `_<<<`,
                // `_<<<<` for 1..4 frames up — and the positional matrix:
                // `_0<<<<`, `_1<<<<`, `_N<<<<` (N positionals × 5 levels).
                // `_0` is canonically aliased to `_` at every level (see
                // `Scope::set_closure_args`).
                //
                // Stryke string-index sugar: `_[N]` (bareword, no sigil) is
                // an alias for `_!N!` — char-of-topic substring. The sigil
                // form `$_[N]` keeps Perl's `@_`-access semantics (first
                // positional arg). We dispatch here, before the generic
                // ArrayElement path, so the AST for `_[N]` carries the
                // synthetic `__topicstr__$NAME` flag the interpreter / VM
                // strip and route to char-of-string.
                if Self::is_underscore_topic_slot(&name) {
                    if matches!(self.peek(), Token::LBracket) && self.peek_line() == line {
                        self.advance(); // [
                        let index = self.parse_expression()?;
                        self.expect(&Token::RBracket)?;
                        return Ok(Expr {
                            kind: ExprKind::ArrayElement {
                                array: format!("__topicstr__{}", name),
                                index: Box::new(index),
                            },
                            line,
                        });
                    }
                    return Ok(Expr {
                        kind: ExprKind::ScalarVar(name.clone()),
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
                    && !(matches!(self.peek(), Token::BitNot)
                        && self.suppress_tilde_range == 0
                        && matches!(
                            self.peek_at(1),
                            Token::Ident(_) | Token::Integer(_) | Token::Float(_)
                        ))
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
                    // Guard: `~Ident` / `~Integer` / `~Float` after a bareword is
                    // the universal-tilde range separator (`I~M~5`, `Mon~Fri`,
                    // `Jan~Dec~2`), not unary BitNot of an arg. Bail to Bareword
                    // so the outer `parse_range` consumes `~` as the range op.
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

    /// `open FH, ...` / `close FH` / `eof FH` — Perl's convention is that an
    /// all-uppercase (letters / digits / `_`, starting with a letter or `_`)
    /// bareword in the filehandle slot is a literal handle name, never a
    /// constant or sub call. This shadows registered constants like `PI`,
    /// `TAU`, `E` and the rare uppercase-letter filehandles (`H`, `O`, …)
    /// that would otherwise route through the bareword resolver.
    ///
    /// Returns `Some(name)` when the next token is such a bareword and the
    /// token after it is one of the accepted terminators (any of `accept`,
    /// or — when `accept` is empty — any of `,`, `;`, `)`, `}`, `|>`, Eof).
    /// Otherwise returns `None` and leaves the cursor untouched.
    fn take_bareword_filehandle_if(&mut self, accept: &[Token]) -> Option<String> {
        let Token::Ident(h) = self.peek().clone() else {
            return None;
        };
        let mut chars = h.chars();
        let first = chars.next()?;
        if !(first.is_ascii_uppercase() || first == '_') {
            return None;
        }
        if !h
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        {
            return None;
        }
        let next = self.peek_at(1);
        let ok = if accept.is_empty() {
            matches!(
                next,
                Token::Comma
                    | Token::Semicolon
                    | Token::RParen
                    | Token::RBrace
                    | Token::Eof
                    | Token::PipeForward
            )
        } else {
            accept
                .iter()
                .any(|t| std::mem::discriminant(t) == std::mem::discriminant(next))
        };
        if !ok {
            return None;
        }
        self.advance();
        Some(h)
    }

    /// `open FH, …` — bareword filehandle followed by a comma.
    fn take_bareword_filehandle(&mut self) -> Option<String> {
        self.take_bareword_filehandle_if(&[Token::Comma])
    }

    /// `close FH` / `eof FH` — bareword filehandle followed by a statement
    /// terminator. Returns a `String` expression to splice into the arg
    /// slot, or `None` if the next token isn't a literal filehandle.
    fn take_bareword_filehandle_arg(&mut self, line: usize) -> Option<Expr> {
        self.take_bareword_filehandle_if(&[]).map(|name| Expr {
            kind: ExprKind::String(name),
            line,
        })
    }

    fn parse_print_like(
        &mut self,
        make: impl FnOnce(Option<String>, Vec<Expr>) -> ExprKind,
    ) -> StrykeResult<Expr> {
        let line = self.peek_line();
        // Check for filehandle: print STDERR "msg"  /  print $fh "msg"
        let handle = if let Token::Ident(ref h) = self.peek().clone() {
            if h.chars().all(|c| c.is_uppercase() || c == '_')
                && !matches!(self.peek(), Token::LParen)
            {
                let h = h.clone();
                let saved = self.pos;
                self.advance();
                // Verify next token is a term start (not operator).
                // Guard: `~Ident` / `~Integer` / `~Float` is a universal-tilde
                // range separator (`p I~M~5`, `p Mon~Fri`), not unary BitNot of
                // an arg. Bail filehandle detection so the bareword `I` flows
                // into the regular expression path where `parse_range` consumes
                // `~` as the range op.
                let is_tilde_range_after = matches!(self.peek(), Token::BitNot)
                    && self.suppress_tilde_range == 0
                    && matches!(
                        self.peek_at(1),
                        Token::Ident(_) | Token::Integer(_) | Token::Float(_)
                    );
                if !is_tilde_range_after
                    && (self.peek().is_term_start()
                        || matches!(
                            self.peek(),
                            Token::DoubleString(_)
                                | Token::BacktickString(_)
                                | Token::SingleString(_)
                        ))
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
            // Exclude tokens on a later line — a newline ends the print statement
            // in stryke, so `p $j\nmy $k = …` must not absorb the following `my`.
            let v = v.clone();
            if v == "_" {
                None
            } else {
                let saved = self.pos;
                let var_line = self.peek_line();
                self.advance();
                let next = self.peek().clone();
                let next_line = self.peek_line();
                let is_stmt_modifier = matches!(&next, Token::Ident(kw)
                    if matches!(kw.as_str(), "if" | "unless" | "while" | "until" | "for" | "foreach"));
                if !is_stmt_modifier
                    && next_line == var_line
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
        // Use `parse_list_until_terminator_allow_pipe` so that `p @a |> sum`
        // parses as `p(sum(@a))`, matching `~>` thread-first behavior.
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
                self.parse_list_until_terminator_allow_pipe()?
            };
        Ok(Expr {
            kind: make(handle, args),
            line,
        })
    }

    fn parse_block_list(&mut self) -> StrykeResult<(Block, Expr)> {
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
    ) -> StrykeResult<(Vec<Expr>, Option<Expr>)> {
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
    ) -> StrykeResult<(Expr, Block, Expr, Option<Expr>)> {
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
    ) -> StrykeResult<(Expr, Block, Expr, Option<Expr>)> {
        // `pmap_on $c { BLOCK } @list` — suppress `$c { ... }` hash-subscript
        // auto-arrow so the brace opens the BLOCK, not a `$c->{...}` deref.
        self.suppress_scalar_hash_brace = self.suppress_scalar_hash_brace.saturating_add(1);
        let cluster = self.parse_assign_expr();
        self.suppress_scalar_hash_brace = self.suppress_scalar_hash_brace.saturating_sub(1);
        let cluster = cluster?;
        // Accept the canonical `pmap_on $c, { BLOCK } @list` LSP-doc form too.
        self.eat(&Token::Comma);
        let block = self.parse_block_or_bareword_block()?;
        let block_end_line = self.prev_line();
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
        ) || (self.in_pipe_rhs()
            && (matches!(self.peek(), Token::Comma) || self.peek_line() > block_end_line));
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
    ) -> StrykeResult<(Block, Expr, Option<Expr>)> {
        let block = self.parse_block_or_bareword_block()?;
        let block_end_line = self.prev_line();
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
        // the enclosing pipe-forward loop. A newline after the block also
        // terminates in pipe-RHS — the LHS supplies the list, so we must NOT
        // greedily eat the next statement (matches `parse_block_list`).
        let empty_list_ok = matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::PipeForward
        ) || (self.in_pipe_rhs()
            && (matches!(self.peek(), Token::Comma) || self.peek_line() > block_end_line));
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
        line: usize,
    ) -> StrykeResult<(Option<Box<Expr>>, Block)> {
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
    fn parse_fan_blockless_body(&mut self, line: usize) -> StrykeResult<Block> {
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
    fn parse_block_or_bareword_block(&mut self) -> StrykeResult<Block> {
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
    fn parse_block_or_bareword_block_no_args(&mut self) -> StrykeResult<Block> {
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
            | "splice_last" | "splice1" | "spl_last"
            | "pack" | "unpack"
            | "unpack_first" | "unpack1" | "up1"
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
            | "fork" | "wait" | "waitpid" | "kill" | "syscall" | "alarm" | "sleep"
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
            // ── numerical stability + modern IDs ───────────────────────────
            | "ulid" | "is_ulid" | "ulid_timestamp"
            | "kahan_sum" | "welford_mean" | "welford_variance"
            | "welford_stddev" | "welford_pop_variance"
            // ── Shell-like REPL (Tier S) ───────────────────────────────────
            | "clear" | "cls" | "whoami" | "groups"
            | "pushd" | "popd" | "dir_stack"
            | "history" | "repl_alias" | "repl_unalias" | "set_alias" | "unset_alias"
            | "term_size" | "term_width" | "term_height"
            | "set_title" | "beep" | "ring_bell" | "man" | "manpage"
            | "run" | "exec_script" | "source" | "src"
            // ── Shell-like REPL (Tier A) ───────────────────────────────────
            | "rm" | "mktemp" | "mktempdir" | "whereis"
            | "nice" | "renice"
            | "tree" | "comm" | "column" | "xargs"
            | "openurl" | "xdg_open"
            | "curl_get" | "curl_post"
            | "iconv" | "strftime"
            | "tac" | "rev_lines"
            | "tty_raw" | "tty_cooked"
            // ── probabilistic data structures ──────────────────────────────
            | "bloom_filter" | "bloom_add" | "bloom_contains" | "bloom_len"
            | "bloom_clear" | "bloom_merge" | "bloom_fpr" | "bloom_bits"
            | "bloom_serialize" | "bloom_deserialize"
            | "hll" | "hyperloglog" | "hll_add" | "hll_count" | "hll_merge"
            | "hll_clear" | "hll_precision" | "hll_serialize" | "hll_deserialize"
            | "cms" | "count_min_sketch" | "cms_add" | "cms_count" | "cms_query"
            | "cms_merge" | "cms_clear" | "cms_serialize" | "cms_deserialize"
            | "topk" | "top_k_sketch" | "topk_add" | "topk_heavies" | "topk_count"
            | "topk_size" | "topk_merge" | "topk_clear"
            | "topk_serialize" | "topk_deserialize"
            | "t_digest" | "tdg" | "tdigest" | "td_add" | "td_quantile" | "td_count"
            | "td_min" | "td_max" | "td_sum" | "td_mean" | "td_merge" | "td_clear"
            | "td_serialize" | "td_deserialize"
            | "roaring" | "roaring_bitmap" | "rbm" | "rb_add" | "rb_remove" | "rb_contains"
            | "rb_len" | "rb_min" | "rb_max" | "rb_to_array" | "rb_rank"
            | "rb_or" | "rb_and" | "rb_xor" | "rb_andnot" | "rb_clear"
            | "rb_serialize" | "rb_deserialize"
            // ── Rate limiters / hash ring / LSH / trees / diff ────────────
            | "token_bucket" | "leaky_bucket" | "rl_try_take" | "rl_available"
            | "hash_ring" | "consistent_hash" | "hr_add" | "hr_remove" | "hr_get" | "hr_nodes"
            | "simhash" | "sh_add" | "sh_digest" | "sh_similarity"
            | "minhash" | "mh_add" | "mh_jaccard" | "mh_merge"
            | "interval_tree" | "it_insert" | "it_query_point" | "it_query_range"
            | "it_remove" | "it_len"
            | "bk_tree" | "bk_insert" | "bk_query" | "bk_len"
            | "rope" | "rope_insert" | "rope_delete" | "rope_substring"
            | "rope_to_string" | "rope_len"
            | "myers_diff" | "patience_diff"
            // ── rkyv KV store ──────────────────────────────────────────────
            | "kv_open" | "kv_new" | "kv_put" | "kv_set" | "kv_get"
            | "kv_del" | "kv_delete" | "kv_remove" | "kv_exists" | "kv_has"
            | "kv_keys" | "kv_scan" | "kv_len" | "kv_count" | "kv_size"
            | "kv_commit" | "kv_flush" | "kv_batch" | "kv_close"
            | "kv_stats" | "kv_info"
            // ── aop ────────────────────────────────────────────────────────
            | "proceed" | "intercept_list" | "intercept_remove" | "intercept_clear"
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
            | "first" | "detect" | "find" | "find_index" | "firstidx" | "first_index"
            | "compact" | "concat" | "chain" | "reject" | "grepv" | "flatten" | "set"
            | "min_by" | "max_by" | "sort_by" | "tally"
            | "each_with_index" | "count" | "cnt" |"len" | "group_by" | "chunk_by"
            | "zip" | "chunk" | "chunked" | "sliding_window" | "windowed"
            | "enumerate" | "with_index" | "shuffle" | "shuffled"| "heap"
            | "take_while" | "drop_while" | "skip_while" | "tap" | "peek" | "partition"
            | "zip_with" | "count_by" | "skip" | "first_or"
            // ── cli / argv ──────────────────────────────────────────────────
            | "getopts"
            // ── pipeline / string helpers ───────────────────────────────────
            | "input" | "lines" | "words" | "chars" | "cindex" | "crindex"
            | "digits" | "letters" | "letters_uc" | "letters_lc"
            | "punctuation" | "punct"
            | "sentences" | "sents"
            | "paragraphs" | "paras" | "sections" | "sects"
            | "numbers" | "nums" | "graphemes" | "grs" | "columns" | "cols"
            | "trim" | "avg" | "stddev"
            | "squared" | "sq" | "square" | "cubed" | "cb" | "cube" | "expt" | "pow" | "pw"
            | "normalize" | "snake_case" | "camel_case" | "kebab_case"
            | "frequencies" | "freq" | "pfrequencies" | "pfreq"
            | "interleave" | "ddump" | "stringify" | "str" | "top"
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
            | "sockets" | "pipes" | "block_devices" | "char_devices" | "exe" | "executables"
            | "basename" | "dirname" | "fileparse" | "realpath" | "canonpath"
            | "copy" | "cp" | "move" | "spurt" | "spit" | "read_bytes" | "which"
            | "getcwd" | "cd" | "ls" | "touch" | "gethostname" | "uname"
            | "file" | "xxd"
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
            // ── github / gh REST API ─────────────────────────────────────────
            | "gh_get" | "gh_user" | "gh_org" | "gh_followers" | "gh_following"
            | "gh_repo" | "gh_repos" | "gh_org_repos" | "gh_starred"
            | "gh_gists" | "gh_gist"
            | "gh_issues" | "gh_prs" | "gh_commits" | "gh_branches"
            | "gh_tags" | "gh_releases" | "gh_contributors" | "gh_forks"
            | "gh_stargazers" | "gh_topics" | "gh_languages"
            | "gh_readme" | "gh_workflows" | "gh_runs"
            | "gh_search_repos" | "gh_search_users" | "gh_search_code" | "gh_search_issues"
            | "gh_rate_limit" | "gh_meta" | "gh_emojis" | "gh_zen"
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
            | "datetime_utc" | "datetime_now_tz" | "now"
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
            // ── caching ────────────────────────────────────────────────────────
            | "cache_clear" | "cache_exists" | "cache_stats" | "cacheview"
            // ── testing framework ────────────────────────────────────────────
            | "assert_eq" | "assert_ne" | "assert_ok" | "assert_err"
            | "assert_true" | "assert_false"
            | "assert_gt" | "assert_lt" | "assert_ge" | "assert_le"
            | "assert_match" | "assert_contains" | "assert_near" | "assert_dies"
            | "test_run" | "run_tests" | "test_skip" | "skip_test" | "skip_assert"
            // ── system info ─────────────────────────────────────────────────
            | "mounts" | "du" | "du_tree" | "process_list"
            | "thread_count" | "pool_info" | "par_bench"
            | "perfview" | "pfv"
            | "docs" | "help" | "h"
            | "banner"
            // ── network / ip / cidr ─────────────────────────────────────────
            | "ip_parse" | "ip_is_valid" | "ip_version" | "ip_family"
            | "ip_to_int" | "int_to_ip" | "ip_to_bytes" | "bytes_to_ip"
            | "ip_to_bits" | "bits_to_ip"
            | "ip_is_private" | "ip_is_loopback" | "ip_is_multicast"
            | "ip_is_link_local" | "ip_is_unspecified" | "ip_is_global"
            | "ip_is_documentation" | "ip_is_benchmarking" | "ip_is_shared"
            | "ip_is_reserved" | "ip_is_broadcast"
            | "ip_canonical" | "ip_reverse" | "ip_arpa"
            | "ip_compare" | "ip_sort" | "ip_random"
            | "ipv4_parse" | "ipv4_is_valid" | "ipv4_classful_class"
            | "ipv6_parse" | "ipv6_is_valid" | "ipv6_canonical"
            | "ipv6_expand" | "ipv6_compress" | "ipv6_strip_zone" | "ipv6_zone_id"
            | "ipv6_link_local" | "ipv6_unique_local" | "ipv6_solicited_node"
            | "ipv6_eui64_addr" | "ipv6_link_local_from_mac"
            | "ipv4_to_ipv6_mapped" | "ipv4_to_ipv6_6to4" | "ipv6_to_ipv4_compat"
            | "ipv6_is_6to4" | "ipv6_6to4_extract"
            | "ipv6_is_teredo" | "ipv6_teredo_extract"
            | "ipv6_is_isatap" | "ipv6_isatap_extract"
            | "cidr_parse" | "cidr_valid_subnet" | "cidr_format"
            | "cidr_prefix_len" | "cidr_class"
            | "cidr_network" | "cidr_broadcast" | "cidr_netmask"
            | "cidr_hostmask" | "cidr_wildcard"
            | "cidr_to_netmask" | "netmask_to_prefix"
            | "cidr_first_host" | "cidr_last_host" | "cidr_num_hosts"
            | "cidr_size" | "cidr_hosts" | "cidr_iterate"
            | "cidr_contains" | "ip_in_cidr" | "ip_in_subnet"
            | "cidr_subnet" | "cidr_supernet" | "cidr_subnets" | "cidr_split"
            | "cidr_overlaps" | "cidr_aggregate" | "cidr_summarize"
            | "cidr_intersection" | "cidr_difference" | "cidr_union"
            | "cidr_minimum_covering" | "cidr_is_aggregable"
            | "cidr_next" | "cidr_prev" | "cidr_distance"
            | "cidr_random_ip" | "ip_random_in_cidr"
            | "cidr_compare" | "cidr_sort"
            | "mac_parse" | "mac_is_valid" | "mac_normalize" | "mac_format"
            | "mac_to_int" | "int_to_mac" | "mac_to_bytes" | "bytes_to_mac"
            | "mac_oui" | "mac_vendor_lookup" | "mac_lookup_vendor"
            | "mac_is_unicast" | "mac_is_multicast" | "mac_is_broadcast"
            | "mac_is_locally_administered" | "mac_is_universally_administered"
            | "mac_random" | "mac_random_local" | "mac_compare"
            | "eui48_to_eui64" | "eui64_to_eui48" | "eui64_from_mac"
            | "port_name" | "port_is_well_known" | "port_is_assigned"
            | "port_is_registered" | "port_is_ephemeral" | "port_is_dynamic"
            | "port_to_service" | "port_service_lookup"
            | "port_parse_range" | "port_random_ephemeral"
            | "ws_handshake_key" | "ws_handshake_accept"
            | "ws_mask" | "ws_unmask" | "ws_frame_encode" | "ws_frame_decode"
            | "ws_close_frame"
            | "cookie_parse" | "cookie_format"
            | "cookie_jar_new" | "cookie_jar_add" | "cookie_jar_get"
            | "cookie_is_session" | "cookie_is_expired"
            | "cookie_domain_matches" | "cookie_path_matches"
            | "cookie_set_max_age"
            | "http_method_is_idempotent" | "http_method_is_safe"
            | "http_method_has_body"
            | "http_status_class"
            | "http_status_is_informational" | "http_status_is_success"
            | "http_status_is_redirect" | "http_status_is_client_error"
            | "http_status_is_server_error"
            | "http_status_text" | "http_date_parse" | "http_date_format"
            | "mime_type_for_extension" | "mime_extension_for_type"
            | "mime_is_text" | "mime_is_image" | "mime_is_audio"
            | "mime_is_video" | "mime_is_application"
            | "bandwidth_format" | "bandwidth_parse"
            | "latency_ms" | "packet_loss" | "jitter_ms"
            | "rtt_min" | "rtt_max" | "rtt_avg"
            // ── validation / input checks ──
            | "is_alpha_only" | "is_alphanumeric_only" | "is_numeric_only"
            | "is_ascii_only" | "is_printable_ascii" | "is_utf8"
            | "is_lowercase" | "is_uppercase" | "is_titlecase"
            | "is_palindrome_str"
            | "is_hex" | "is_octal" | "is_binary" | "is_base32"
            | "is_md5_hash" | "is_sha1_hash" | "is_sha256_hash"
            | "is_ipv6" | "is_cidr" | "is_mac"
            | "is_url_http" | "is_url_https"
            | "is_uuid_v4" | "is_uuid_v7"
            | "is_jwt" | "is_email_strict"
            | "luhn_digit" | "is_imei" | "is_imsi"
            | "is_vin" | "vin_decode"
            | "is_ean13" | "is_upc"
            | "is_isbn" | "isbn10_to_isbn13" | "isbn13_to_isbn10"
            | "iban_format" | "iban_country" | "is_bic" | "is_swift"
            | "is_phone" | "is_phone_e164"
            | "is_zip_us" | "is_zip_plus4" | "is_postal_code" | "is_ssn_us"
            | "semver_compare" | "semver_satisfies"
            | "semver_increment_major" | "semver_increment_minor" | "semver_increment_patch"
            // ── math / number theory extras ──
            | "extended_gcd" | "modinverse" | "modpow" | "modular_sqrt"
            | "stirling_1" | "stirling_2" | "catalan_number" | "lucas_n"
            | "prime_count_below" | "divisor_count" | "divisor_sum" | "sigma_divisors"
            | "sum_digits" | "product_digits" | "collatz_steps"
            | "hyperoperation" | "busy_beaver"
            | "quadratic_residue" | "is_quadratic_residue"
            | "discrete_log" | "order_modulo" | "square_free"
            | "perfect_number" | "abundant" | "deficient"
            // ── random / sampling extras ──
            | "random_bernoulli" | "random_normal" | "random_lognormal"
            | "random_exponential" | "random_poisson" | "random_gamma" | "random_beta"
            | "random_alphanumeric" | "random_alphabetic" | "random_password"
            | "random_choices_weighted"
            | "sample_weighted_unique" | "reservoir_sample_weighted"
            | "seeded_rng" | "save_random_state" | "restore_random_state"
            // ── complex / geom / color / trig ──
            | "complex_new" | "complex_real" | "complex_imag"
            | "complex_polar" | "complex_from_polar"
            | "complex_magnitude" | "complex_abs" | "complex_phase" | "complex_angle"
            | "complex_conjugate"
            | "complex_add" | "complex_sub" | "complex_mul" | "complex_div"
            | "complex_pow" | "complex_sqrt" | "complex_exp" | "complex_log"
            | "complex_sin" | "complex_cos" | "complex_tan"
            | "complex_sinh" | "complex_cosh" | "complex_tanh"
            | "complex_equal"
            | "point_angle"
            | "line_intersect" | "line_segment_intersect" | "line_distance_point"
            | "polygon_signed_area" | "polygon_orientation" | "polygon_reverse"
            | "polygon_contains_point" | "polygon_convex"
            | "polygon_simplify_dp" | "polygon_convex_hull_2d"
            | "triangle_area" | "triangle_centroid"
            | "triangle_circumcircle" | "triangle_incircle"
            | "triangle_contains_point"
            | "circle_circumference" | "circle_area"
            | "circle_intersects_line" | "circle_intersects_circle"
            | "rect_area" | "rect_perimeter" | "rect_intersect"
            | "rect_contains_point" | "rect_union"
            | "ellipse_area"
            | "sphere_surface_area" | "cylinder_surface_area"
            | "cone_surface_area" | "torus_surface_area"
            | "srgb_to_rgb" | "rgb_to_srgb"
            | "rgb_to_p3" | "p3_to_rgb"
            | "rgb_to_adobe_rgb" | "adobe_rgb_to_rgb"
            | "xyz_d65_to_d50" | "xyz_d50_to_d65"
            | "gamma_apply" | "gamma_remove"
            | "white_point_d65" | "white_point_d50"
            | "color_temperature_to_rgb" | "rgb_to_color_temperature"
            | "chromatic_adaptation"
            | "color_interpolate_rgb" | "color_interpolate_hsl"
            | "color_interpolate_lab" | "color_interpolate_oklab"
            | "color_blend_screen"
            | "atan2_deg" | "atan2_quadrant"
            | "polar_to_cartesian" | "cartesian_to_polar"
            | "spherical_to_cartesian" | "cartesian_to_spherical"
            | "cylindrical_to_cartesian" | "cartesian_to_cylindrical"
            | "versine_fn"
            // ── iterator + string-distance extras ──
            | "triples" | "n_tuples" | "peekable" | "runs" | "unique_by"
            | "multipeek" | "lookahead_n"
            | "sliding_average" | "sliding_sum" | "sliding_max" | "sliding_min"
            | "top_n_by" | "bottom_n_by" | "all_equal" | "take_n_random"
            | "unzip3" | "roundrobin" | "mode_iter" | "distinct_sample"
            | "ranked_choice" | "boyer_moore_majority"
            | "quickselect_nth" | "quickselect_median"
            | "top_k_min_heap" | "bottom_k_max_heap"
            | "unique_consecutive" | "exclude" | "exclude_first" | "exclude_last"
            | "weave_n" | "pad_left_n" | "pad_right_n"
            | "collect_into_string" | "collect_into_hashset" | "collect_into_btreeset"
            | "collect_into_hashmap" | "collect_into_btreemap"
            | "foldl1_iter" | "foldr1_iter"
            | "sort_by_cached_key"
            | "position_max" | "position_min" | "position_max_by" | "position_min_by"
            | "group_map"
            | "levenshtein_normalized" | "ratcliff_obershelp" | "match_rating"
            | "str_lcs" | "str_lcs_length" | "str_longest_common_substring"
            | "str_kmp" | "str_boyer_moore" | "str_rabin_karp"
            | "str_aho_corasick" | "str_z_array" | "str_suffix_array"
            | "str_rotations" | "str_compress_rle" | "str_decompress_rle"
            | "str_huffman_encode" | "str_huffman_decode"
            | "str_compress_lzss" | "str_decompress_lzss"
            | "str_isogram" | "fold_case"
            // ── extras ──
            | "bignum_new" | "bignum_from_str" | "bignum_to_str" | "bignum_to_int"
            | "bignum_add" | "bignum_sub" | "bignum_mul" | "bignum_div" | "bignum_mod"
            | "bignum_pow" | "bignum_modpow" | "bignum_gcd" | "bignum_lcm"
            | "bignum_factorial" | "bignum_sqrt" | "bignum_bit_length"
            | "bignum_set_bit" | "bignum_clear_bit" | "bignum_test_bit"
            | "bignum_and" | "bignum_or" | "bignum_xor" | "bignum_not"
            | "bignum_shl" | "bignum_shr" | "bignum_compare"
            | "bignum_negate" | "bignum_abs" | "bignum_sign"
            | "bignum_is_zero" | "bignum_is_negative" | "bignum_is_prime"
            | "bignum_random"
            | "gravity_constant" | "physics_apply_force" | "physics_apply_impulse"
            | "physics_collide_aabb" | "physics_collide_sphere"
            | "physics_raycast" | "physics_step"
            | "particle_emit" | "particle_update"
            | "vector2_new" | "vector2_add" | "vector2_sub" | "vector2_scale"
            | "vector2_dot" | "vector2_cross" | "vector2_length"
            | "vector2_normalize" | "vector2_distance" | "vector2_rotate"
            | "quaternion_new" | "quaternion_from_axis_angle"
            | "quaternion_multiply" | "quaternion_normalize" | "quaternion_to_matrix"
            | "freq_to_note" | "note_to_freq" | "midi_note_to_name"
            | "chord_notes" | "scale_notes" | "transpose_note"
            | "window_tukey" | "zero_crossing_rate" | "peak_db"
            | "audio_normalize" | "audio_fade_in" | "audio_fade_out"
            | "audio_to_mono" | "audio_to_stereo"
            | "biquad_lowpass" | "biquad_highpass" | "biquad_bandpass" | "biquad_notch"
            | "oscillator_sine" | "oscillator_square"
            | "oscillator_sawtooth" | "oscillator_triangle"
            | "adsr_envelope" | "ar_envelope" | "crossfade"
            | "fade_curve_linear" | "fade_curve_logarithmic" | "fade_curve_exponential"
            | "bbox_contains" | "bbox_union" | "bbox_intersect"
            | "bbox_center" | "bbox_area"
            | "mercator_unproject" | "geohash_precision"
            // ── extras ──
            | "jq_get" | "jq_set" | "jq_delete" | "jq_select"
            | "jq_keys_at" | "jq_values_at" | "jq_length_at"
            | "jq_type" | "jq_has" | "jq_paths" | "jq_leaf_paths"
            | "jq_walk" | "jq_map_values" | "jq_filter"
            | "jq_to_entries" | "jq_from_entries" | "jq_with_entries"
            | "jq_recurse" | "jq_min_by" | "jq_max_by"
            | "jq_sort_by" | "jq_group_by" | "jq_unique_by"
            | "jq_any" | "jq_all" | "jq_flatten"
            | "jq_index" | "jq_indices" | "jq_first" | "jq_last"
            | "jq_split_at" | "jq_chunks" | "jq_zip" | "jq_combinations"
            | "json_diff" | "json_patch" | "json_merge_patch"
            | "json_pointer_resolve" | "json_pointer_set"
 | "html_to_text" | "html_pretty" | "html_minify"
            | "html_sanitize" | "html_strip_tags" | "html_strip_scripts" | "html_strip_styles"
            | "html_extract_links" | "html_extract_images" | "html_extract_text"
            | "html_extract_meta" | "html_extract_title"
            | "html_extract_headings" | "html_extract_tables"
            | "html_inner_text" | "html_canonical_url"
            | "html_meta_charset" | "html_meta_keywords" | "html_meta_description"
            | "html_meta_og" | "html_meta_twitter"
            | "html_to_markdown" | "markdown_to_html" | "markdown_render"
 | "xml_pretty" | "xml_minify"
            | "xml_namespace" | "xml_text" | "xml_attrs"
            | "xml_children_by_tag" | "xml_root"
            | "xpath_select_one" | "xpath_attribute" | "xpath_text"
            | "xml_to_json" | "json_to_xml" | "xml_canonicalize"
            | "css_parse" | "css_minify" | "css_pretty"
            | "css_selector_parse" | "css_rule_extract" | "css_specificity"
            | "css_var_resolve" | "css_property_set" | "css_property_get"
            | "css_url_extract" | "css_import_extract" | "css_font_extract"
            | "selector_to_xpath" | "xpath_to_selector"
            // ── extras ──
            | "http_status_continue" | "http_status_switching_protocols"
            | "http_status_ok" | "http_status_created" | "http_status_accepted"
            | "http_status_no_content" | "http_status_partial_content"
            | "http_status_multiple_choices" | "http_status_moved_permanently"
            | "http_status_found" | "http_status_see_other" | "http_status_not_modified"
            | "http_status_temporary_redirect" | "http_status_permanent_redirect"
            | "http_status_bad_request" | "http_status_unauthorized"
            | "http_status_payment_required" | "http_status_forbidden"
            | "http_status_not_found" | "http_status_method_not_allowed"
            | "http_status_not_acceptable" | "http_status_conflict" | "http_status_gone"
            | "http_status_length_required" | "http_status_precondition_failed"
            | "http_status_payload_too_large" | "http_status_uri_too_long"
            | "http_status_unsupported_media_type" | "http_status_range_not_satisfiable"
            | "http_status_expectation_failed" | "http_status_im_a_teapot"
            | "http_status_unprocessable_entity" | "http_status_too_many_requests"
            | "http_status_internal_server_error" | "http_status_not_implemented"
            | "http_status_bad_gateway" | "http_status_service_unavailable"
            | "http_status_gateway_timeout" | "http_status_http_version_not_supported"
            | "http_method_get" | "http_method_post" | "http_method_put"
            | "http_method_delete" | "http_method_patch" | "http_method_head"
            | "http_method_options" | "http_method_trace" | "http_method_connect"
            | "dbeta" | "qbeta" | "rbeta" | "dcauchy" | "qcauchy" | "rcauchy"
            | "dexp" | "qexp" | "rexp" | "dgamma" | "qgamma" | "rgamma"
            | "dlnorm" | "qlnorm" | "rlnorm" | "dlogis" | "qlogis" | "rlogis"
            | "dpois" | "qpois" | "rpois" | "dweibull" | "qweibull" | "rweibull"
            | "qnorm" | "rnorm" | "qunif" | "runif"
            | "qbinom" | "rbinom" | "qgeom" | "rgeom" | "qhyper" | "rhyper"
            | "qchisq" | "rchisq" | "qf" | "rf" | "qt" | "rt"
            // ── extras ──
            | "currency_format" | "currency_parse" | "currency_round"
            | "currency_split_thousands" | "currency_code_to_symbol"
            | "currency_symbol_to_code" | "currency_convert" | "currency_rate"
            | "currency_iso_4217" | "currency_decimal_places"
            | "money_add" | "money_sub" | "money_mul" | "money_div" | "money_compare"
            | "tokenize_simple" | "tokenize_word" | "tokenize_subword"
            | "tokenize_bpe" | "tokenize_sentencepiece" | "embed_text"
            | "cosine_similarity" | "euclidean_distance" | "manhattan_distance"
            | "dot_product" | "normalize_vector"
            | "vector_add" | "vector_sub" | "vector_scale" | "vector_mean"
            | "top_k_indices" | "softmax" | "sigmoid" | "log_softmax" | "cross_entropy"
            | "path_canonical" | "path_relative_to" | "path_components"
            | "path_filename" | "path_stem" | "path_extension"
            | "path_join_many" | "path_with_extension" | "path_with_filename"
            | "path_is_subdirectory" | "path_common_ancestor" | "path_strip_prefix"
            | "path_glob_match_regex"
            | "file_mime" | "file_kind" | "file_attr_get" | "file_attr_set"
            | "xattr_get" | "xattr_set" | "xattr_list"
            | "file_chmod_string" | "file_chmod_octal" | "file_locked"
            | "file_acl_get" | "file_acl_set"
            | "locale_parse" | "locale_format" | "locale_language"
            | "locale_region" | "locale_script" | "locale_variant" | "locale_canonical"
            | "bcp47_parse" | "bcp47_format" | "bcp47_validate"
            | "language_tag_match" | "language_tag_subtags"
            | "locale_likely_subtags" | "locale_minimize" | "locale_collation"
            | "locale_calendar" | "locale_currency"
            | "locale_number_format" | "locale_date_format" | "locale_time_format"
            | "locale_decimal_separator" | "locale_group_separator"
            | "locale_first_day_of_week" | "locale_measurement_system"
            | "country_code_alpha2" | "country_code_alpha3" | "country_code_numeric"
            | "country_name" | "country_phone_prefix" | "country_currency"
            | "country_languages"
            | "language_iso_639_1" | "language_iso_639_2" | "language_iso_639_3"
            | "language_name"
            | "channel_unbounded" | "channel_bounded" | "channel_sync"
            | "channel_send_timeout" | "channel_recv_timeout"
            | "channel_try_recv" | "channel_try_send"
            | "channel_drain" | "channel_close" | "channel_is_closed"
            | "broadcast_channel_new" | "broadcast_channel_subscribe"
            | "broadcast_channel_publish"
            | "mpsc_new" | "mpmc_new" | "spmc_new" | "oneshot_new"
            // ── mutex + counting semaphore ─────────────────────────────────
            | "mutex" | "mutex_lock" | "mutex_unlock" | "mutex_try_lock" | "mutex_is_locked"
            | "semaphore" | "sem"
            | "semaphore_acquire" | "sem_acquire"
            | "semaphore_release" | "sem_release"
            | "semaphore_try_acquire" | "sem_try_acquire"
            | "semaphore_permits" | "sem_permits"
            | "semaphore_limit" | "sem_limit"
            // ── stress testing ──────────────────────────────────────────────
            | "stress_cpu" | "scpu" | "stress_mem" | "smem"
            | "stress_io" | "sio" | "stress_test" | "st"
            | "heat" | "fire" | "fire_and_forget" | "pin"
            // ── I/O extensions ──────────────────────────────────────────────
            | "slurp" | "cat" | "c" | "capture" | "pager" | "pg" | "less"
            | "stdin"
            // ── internal ────────────────────────────────────────────────────
            | "__stryke_rust_compile"
            | "vec_set_value"
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
            | "hash_map_values" | "hash_filter_keys" | "hash_filter_values"
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
            | "path_ext" | "path_parent" | "path_join" | "path_split"
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
            // ── symbol table ────────────────────────────────────────────────
            | "refresh_stashes"
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
            // ── trig / math ───────────────────────────────────────
            | "tan" | "asin" | "acos" | "atan"
            | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh"
            | "sqr" | "cube_fn"
            | "mod_op" | "ceil_div" | "floor_div"
            | "is_finite" | "is_infinite" | "is_inf" | "is_nan"
            | "degrees" | "radians"
            | "min_abs" | "max_abs"
            | "saturate" | "sat01" | "wrap_around"
            // ── string ────────────────────────────────────────────
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
            // ── predicates ────────────────────────────────────────
            | "is_pair" | "is_triple"
            | "is_sorted" | "is_asc" | "is_sorted_desc" | "is_desc"
            | "is_empty_arr" | "is_empty_hash"
            | "is_subset" | "is_superset" | "is_permutation"
            // ── collection ────────────────────────────────────────
            | "first_eq" | "last_eq"
            | "index_of" | "last_index_of" | "positions_of"
            | "batch" | "binary_search" | "bsearch" | "linear_search" | "lsearch"
            | "distinct_count" | "longest" | "shortest"
            | "array_union" | "list_union"
            | "array_intersection" | "list_intersection"
            | "array_difference" | "list_difference"
            | "symmetric_diff" | "group_of_n" | "chunk_n"
            | "repeat_list" | "cycle_n" | "random_sample" | "sample_n"
            // ── hash ops ──────────────────────────────────────────
            | "pick_keys" | "pick" | "omit_keys" | "omit"
            | "map_keys_fn" | "map_values_fn"
            | "hash_size" | "hash_from_pairs" | "pairs_from_hash"
            | "hash_eq" | "keys_sorted" | "values_sorted" | "remove_keys"
            // ── date ──────────────────────────────────────────────
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
            | "zip_longest" | "zipl" | "zip_fill" | "zipf" | "combinations" | "comb" | "permutations" | "perm"
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
 | "dotp" | "cross_product" | "crossp"
            | "matrix_mul" | "matmul" | "mm"
            | "magnitude" | "mag" | "normalize_vec" | "nrmv"
            | "distance" | "dist" | "mdist"
            | "covariance" | "cov" | "correlation" | "corr"
            | "iqr" | "quantile" | "qntl" | "quantiles" | "qntls"
            | "lsp_completion_words" | "lsp_words"
            | "doctor" | "health"
            | "clamp_int" | "clpi"
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
            // ── Extended stdlib: Array Analysis ──────────────────────
            | "longest_run" | "lrun" | "longest_increasing" | "linc"
            | "longest_decreasing" | "ldec" | "max_sum_subarray" | "maxsub"
            | "majority_element" | "majority" | "kth_largest" | "kthl"
            | "kth_smallest" | "kths" | "count_inversions" | "cinv"
            | "is_monotonic" | "ismono" | "equilibrium_index" | "eqidx"
            // ── Extended stdlib: Set Operations ──────────────────────
            | "jaccard_index" | "jaccard" | "dice_coefficient" | "dicecoef"
            | "overlap_coefficient" | "overlapcoef"
            | "power_set" | "powerset" | "cartesian_power" | "cartpow"
            // ── Extended stdlib: Advanced String ─────────────────────
            | "is_isogram" | "isiso" | "is_heterogram" | "ishet"
            | "hamdist" | "jaro_similarity" | "jarosim"
            | "longest_common_substring" | "lcsub"
            | "longest_common_subsequence" | "lcseq"
            | "count_words" | "wcount" | "count_lines" | "lcount"
            | "count_chars" | "ccount" | "count_bytes" | "bcount"
            // ── Extended stdlib: More Math ───────────────────────────
            | "binomial" | "binom" | "catalan" | "catn" | "pascal_row" | "pascrow"
            | "is_coprime" | "iscopr" | "euler_totient" | "etot"
            | "mobius" | "mob" | "is_squarefree" | "issqfr"
            | "digital_root" | "digroot" | "is_narcissistic" | "isnarc"
            | "is_harshad" | "isharsh" | "is_kaprekar" | "iskap"
            // ── Extended stdlib: Date/Time Additional ────────────────
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
            | "after_n" | "before_n" | "clamp_list" | "normalize_list"

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
            // ── trivial numeric helpers ─────────────────────────────
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
 | "cube_root" | "entropy" | "float_bits" | "fma"
            | "int_bits" | "jaccard_similarity" | "log_base" | "mae" | "mse" | "nth_root"
            | "r_squared" | "reciprocal" | "relu" | "rmse" | "rotate_point" | "round_to"
 | "signum" | "square_root"
            // ── sequences ─────────────────────────────────────────────────────
            | "cubes_seq" | "fibonacci_seq" | "powers_of_seq" | "primes_seq"
            | "squares_seq" | "triangular_seq"
            // ── string helpers ──────────────────────────────────────
            | "alternate_case" | "angle_bracket" | "bracket" | "byte_length"
            | "bytes_to_hex_str" | "camel_words" | "char_length" | "chars_to_string"
            | "chomp_str" | "chop_str" | "filter_chars" | "from_csv_line" | "hex_to_bytes"
            | "insert_str" | "intersperse_char" | "ljust" | "map_chars" | "mirror_string"
            | "normalize_whitespace" | "only_alnum" | "only_alpha" | "only_ascii"
            | "only_digits" | "parenthesize" | "remove_str" | "repeat_string" | "rjust"
            | "sentence_case" | "string_count" | "string_sort" | "string_to_chars"
            | "string_unique_chars" | "substring" | "to_csv_line" | "trim_left" | "trim_right"
            | "xor_strings"
            // ── list helpers ─────────────────────────────────────────
            | "adjacent_difference" | "append_elem" | "consecutive_pairs" | "contains_elem"
            | "count_elem" | "drop_every" | "duplicate_count" | "elem_at" | "find_first"
            | "first_elem" | "flatten_once" | "fold_left" | "from_digits" | "from_pairs"
            | "group_by_size" | "hash_from_list"
            | "hash_merge_deep" | "hash_to_list" | "hash_zip" | "head_n" | "histogram_bins"
            | "index_of_elem" | "init_list" | "interleave_lists" | "last_elem" | "least_common"
            | "list_compact" | "list_eq" | "list_flatten_deep" | "max_list" | "mean_list"
            | "min_list" | "mode_list" | "most_common" | "partition_two" | "prefix_sums"
            | "prepend" | "product_list" | "remove_at" | "remove_elem" | "remove_first_elem"
            | "repeat_elem" | "running_max" | "running_min" | "sample_one" | "scan_left"
            | "second_elem" | "span" | "suffix_sums" | "sum_list" | "tail_n" | "take_every"
            | "third_elem" | "to_array" | "to_pairs" | "trimmed_mean" | "unique_count_of"
            | "wrap_index" | "digits_of"
            // ── predicates ──────────────────────────────────────────
            | "all_match" | "any_match" | "is_between" | "is_blank_or_nil" | "is_divisible_by"
            | "is_email" | "is_even" | "is_falsy" | "is_fibonacci" | "is_hex_color"
            | "is_in_range" | "is_ipv4" | "is_multiple_of" | "is_negative" | "is_nil"
            | "is_nonzero" | "is_odd" | "is_perfect_square" | "is_positive" | "is_power_of"
            | "is_prefix" | "is_present" | "is_strictly_decreasing" | "is_strictly_increasing"
            | "is_suffix" | "is_triangular" | "is_truthy" | "is_url" | "is_whole" | "is_zero"
            // ── counters ────────────────────────────────────────────
            | "count_digits" | "count_letters" | "count_lower" | "count_match"
            | "count_punctuation" | "count_spaces" | "count_upper" | "defined_count"
            | "empty_count" | "falsy_count" | "nonempty_count" | "numeric_count"
            | "truthy_count" | "undef_count"
            // ── conversion / utility ────────────────────────────────
            | "assert_type" | "between" | "clamp_each" | "die_if" | "die_unless"
            | "join_colons" | "join_commas" | "join_dashes" | "join_dots" | "join_lines"
            | "join_pipes" | "join_slashes" | "join_spaces" | "join_tabs" | "measure"
            | "max_float" | "min_float" | "noop_val" | "nop" | "pass" | "pred" | "succ"
            | "tap_debug" | "to_bool" | "to_float" | "to_int" | "to_string" | "void"
            | "range_exclusive" | "range_inclusive"
            // ── math / numeric extras ─────────────────────────────────────────
            | "aliquot_sum" | "autocorrelation" | "bell_number" | "cagr" | "coeff_of_variation"
            | "collatz_length" | "collatz_sequence" | "convolution"
            | "depreciation_double" | "depreciation_linear" | "discount" | "divisors"
            | "epsilon" | "euler_number" | "exponential_moving_average"
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
            | "circle_from_three_points" | "circ3" | "convex_hull" | "ellipse_perimeter" | "ellper"
            | "frustum_volume" | "haversine_distance" | "line_intersection"
            | "point_in_polygon" | "pip" | "polygon_perimeter" | "polyper" | "pyramid_volume"
            | "reflect_point" | "scale_point" | "sector_area"
            | "torus_surface" | "torus_volume" | "translate_point"
            | "vector_angle" | "vector_cross" | "vector_dot" | "vector_magnitude" | "vector_normalize"
            // ── constants ───────────────────────────────────────────────────────
            | "avogadro_number" | "boltzmann_constant" | "electron_mass" | "elementary_charge"
            | "gravitational_constant" | "phi" | "pi" | "PI" | "planck_constant"
            | "proton_mass" | "sol" | "tau" | "TAU" | "E"
            // ── finance ─────────────────────────────────────────────────────────
            | "bac_estimate" | "bmi" | "break_even" | "margin" | "markup" | "roi" | "tax" | "tip"
            // ── finance (extended) ────────────────────────────────────────────
            | "amortization_schedule" | "black_scholes_call" | "black_scholes_put"
            | "bond_price" | "bond_yield" | "capm" | "continuous_compound" | "ccomp"
            | "discounted_payback" | "duration" | "irr"
            | "max_drawdown" | "mdd" | "modified_duration" | "mod_dur" | "nper" | "num_periods" | "payback_period"
            | "pmt" | "pv" | "rule_of_72" | "sharpe_ratio" | "sortino_ratio"
            | "wacc" | "xirr"
            // ── string processing extras ──────────────────────────────────────
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
            // ── matrix operations extras ──────────────────────────────────────
            | "matrix_flatten" | "matrix_from_rows" | "matrix_hadamard" | "matrix_inverse"
            | "matrix_map" | "matrix_max" | "matrix_min" | "matrix_power" | "matrix_sum"
            | "matrix_transpose"
            // ── array / list operations extras ────────────────────────────────
            | "binary_insert" | "bucket" | "clamp_array" | "group_consecutive_by"
            | "histogram" | "merge_sorted" | "next_permutation" | "normalize_array"
            | "normalize_range" | "peak_detect" | "range_compress" | "range_expand"
            | "reservoir_sample" | "run_length_decode_str" | "run_length_encode_str"
            | "zero_crossings"
            // ── DSP / signal (extended) ───────────────────────────────────────
            | "apply_window" | "bandpass_filter" | "cross_correlation" | "dft"
            | "downsample" | "decimate" | "energy" | "envelope" | "hilbert_env" | "highpass_filter" | "idft"
            | "lowpass_filter" | "median_filter" | "normalize_signal" | "phase_spectrum"
            | "power_spectrum" | "psd" | "resample" | "spectral_centroid" | "spectrogram" | "stft" | "upsample" | "interpolate"
            | "window_blackman" | "window_hamming" | "window_hann" | "window_kaiser"
            // ── validation predicates extras ──────────────────────────────────
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
            // ── Wolfram-Math parity: Bessel/Airy/Hankel/Struve/Kelvin ─────
            | "bessel_j" | "bessel_y" | "bessel_i" | "bessel_k"
            | "hankel_h1" | "hankel_h2" | "bessel_j_zero"
            | "airy_ai" | "airy_bi" | "airy_ai_prime" | "airy_bi_prime"
            | "spherical_bessel_j" | "spherical_bessel_y"
            | "struve_h" | "struve_l" | "kelvin_ber" | "kelvin_bei"
            // ── orthogonal polynomials ────────────────────────────────────
            | "legendre_p" | "legendre_q" | "assoc_legendre_p"
            | "hermite_h" | "hermite_he" | "laguerre_l" | "assoc_laguerre_l"
            | "jacobi_p" | "gegenbauer_c" | "chebyshev_t" | "chebyshev_u"
            | "spherical_harmonic_y" | "zernike_r"
            // ── elliptic integrals + Jacobi/Weierstrass/theta ─────────────
            | "elliptic_k" | "elliptic_e" | "elliptic_pi" | "elliptic_f"
            | "elliptic_e_inc" | "elliptic_pi_inc"
            | "carlson_rf" | "carlson_rd" | "carlson_rj"
            | "jacobi_sn" | "jacobi_cn" | "jacobi_dn" | "jacobi_am"
            | "elliptic_theta"
            | "weierstrass_p" | "weierstrass_zeta" | "weierstrass_sigma"
            // ── zeta / polylog / Lerch ────────────────────────────────────
            | "zeta" | "riemann_zeta" | "hurwitz_zeta"
            | "polylog" | "dilog" | "lerch_phi"
            | "riemann_siegel_z" | "riemann_siegel_theta"
            | "dirichlet_eta" | "dirichlet_beta"
            // ── hypergeometric ────────────────────────────────────────────
            | "hypergeometric_2f1" | "hyper_2f1"
            | "hypergeometric_1f1" | "hyper_1f1" | "kummer_m"
            | "hypergeometric_0f1" | "hyper_0f1"
            | "hypergeometric_pfq" | "hyper_pfq"
            | "hypergeometric_u" | "tricomi_u"
            // ── modular forms ─────────────────────────────────────────────
            | "dedekind_eta" | "klein_j" | "klein_invariant_j"
            | "modular_lambda" | "ramanujan_tau"
            // ── integrals: Si / Ci / Ei / Li / Fresnel ────────────────────
            | "sin_integral" | "si_int" | "cos_integral" | "ci_int"
            | "sinh_integral" | "shi_int" | "cosh_integral" | "chi_int"
            | "exp_integral_e" | "ei_n" | "exp_integral_ei" | "ei_int"
            | "log_integral" | "li_int" | "fresnel_s" | "fresnel_c"
            // ── number-theory gaps ────────────────────────────────────────
            | "jacobi_symbol" | "kronecker_symbol"
            | "primitive_root" | "multiplicative_order"
            | "mangoldt_lambda" | "von_mangoldt" | "carmichael_lambda"
            | "squares_r" | "thue_morse" | "rudin_shapiro"
            | "farey_sequence" | "farey"
            | "frobenius_number" | "frobenius_solve" | "stern_brocot"
            // ── combinatorial gaps ────────────────────────────────────────
            | "stirling_s1" | "stirling_first" | "bell_polynomial_b" | "bell_y"
            | "clebsch_gordan" | "three_j_symbol" | "wigner_3j"
            | "six_j_symbol" | "wigner_6j" | "nine_j_symbol" | "wigner_9j"
            | "debruijn_sequence" | "debruijn" | "wigner_d"
            // ── q-series, Mittag-Leffler, Coulomb wave ────────────────────
            | "q_pochhammer" | "q_factorial" | "q_binomial"
            | "q_hypergeometric_pfq"
            | "mittag_leffler_e" | "mittag_leffler"
            | "coulomb_wave_f" | "coulomb_wave_g"
            // ── inverse special functions ─────────────────────────────────
            | "inverse_erf" | "erfinv" | "inverse_erfc" | "erfcinv"
            | "inverse_gamma_regularized" | "gamma_lr_inv"
            | "inverse_beta_regularized" | "beta_reg_inv"
            | "inverse_jacobi_sn"
            // ── piecewise / symbolic primitives ───────────────────────────
            | "dirac_delta" | "heaviside_theta" | "heaviside"
            | "unit_box" | "unit_triangle"
            | "square_wave" | "triangle_wave" | "sawtooth_wave" | "dirac_comb"
            // ── Tier A: number theory extensions ──────────────────────────
            | "liouville_lambda" | "jordan_totient" | "ramanujan_sum"
            | "cyclotomic_polynomial" | "cyclotomic" | "legendre_symbol"
            | "pythagorean_triple_q" | "gen_pythagorean_triple"
            | "sophie_germain_q" | "mersenne_q"
            | "lucas_lehmer_test" | "lucas_lehmer"
            | "continued_fraction" | "from_continued_fraction" | "convergents"
            | "best_rational_approximation" | "best_rational"
            // ── Tier B: combinatorial sequences ───────────────────────────
            | "motzkin_number" | "motzkin"
            | "narayana_number" | "narayana"
            | "delannoy_number" | "delannoy"
            | "schroder_number" | "schroder" | "large_schroder"
            | "small_schroder_number" | "small_schroder"
            | "eulerian_number"
            | "bernoulli_polynomial" | "euler_polynomial"
            | "pell_number" | "pell" | "pell_lucas_number" | "pell_lucas"
            | "perrin_number" | "perrin" | "padovan_number" | "padovan"
            // ── Tier C: linear algebra extras ─────────────────────────────
            | "kronecker_product" | "tensor_product" | "tensor_contract"
            | "matrix_rank" | "mrank"
            | "companion_matrix" | "companion"
            | "characteristic_polynomial" | "charpoly"
            | "singular_values" | "svals"
            | "nullspace" | "null_space" | "kernel"
            // ── Tier D: polynomial algebra ────────────────────────────────
            | "polynomial_gcd" | "polygcd"
            | "polynomial_quotient" | "polyquot"
            | "polynomial_remainder" | "polyrem"
            | "polynomial_resultant" | "resultant"
            | "polynomial_discriminant" | "discriminant"
            | "polynomial_roots" | "polyroots"
            // ── Tier E: more distributions ────────────────────────────────
            | "gumbel_pdf" | "gumbel_cdf" | "gumbel_quantile"
            | "frechet_pdf" | "frechet_cdf" | "frechet_quantile"
            | "logistic_pdf" | "logistic_cdf" | "logistic_quantile"
            | "rayleigh_pdf" | "rayleigh_cdf" | "rayleigh_quantile"
            | "inverse_gamma_pdf" | "inverse_gamma_cdf" | "inverse_gamma_quantile"
            | "kumaraswamy_pdf" | "kumaraswamy_cdf" | "kumaraswamy_quantile"
            // ── Tier F: Mathieu ───────────────────────────────────────────
            | "mathieu_a" | "mathieu_characteristic_a"
            | "mathieu_ce" | "mathieu_se"
            // ── Tier G: Heun general ──────────────────────────────────────
            | "heun_g"
            // ── Tier H: wavelets ──────────────────────────────────────────
            | "haar_transform" | "haar" | "haar_inverse" | "ihaar"
            | "daubechies_db4" | "db4" | "daubechies_db4_inverse" | "idb4"
            // ── Tier I: graph algorithms ──────────────────────────────────
            | "topo_sort_adj"
            | "scc_tarjan" | "tarjan_scc" | "strongly_connected"
            | "bipartite_q" | "is_bipartite"
            | "max_flow_edmonds_karp" | "max_flow" | "edmonds_karp"
            | "min_cut" | "eccentricity"
            | "graph_diameter" | "graph_radius"
            // ── Tier J: misc fillers ──────────────────────────────────────
            | "stieltjes_constant" | "stieltjes"
            | "gauss_sum" | "kloosterman_sum"
            | "eta_quotient" | "root_approximant"
            // ── vector calculus ──────────────────────────────────
            | "numerical_gradient" | "ngrad"
            | "numerical_jacobian" | "njac"
            | "numerical_hessian" | "nhess"
            | "numerical_divergence" | "ndiv"
            | "numerical_curl" | "ncurl"
            | "numerical_laplacian" | "nlap"
            // ── optimization ─────────────────────────────────────
            | "nelder_mead" | "simplex_min"
            | "gradient_descent" | "gd_min"
            | "bfgs_minimize" | "bfgs"
            | "levenberg_marquardt" | "lev_marq" | "lm_min"
            | "conjugate_gradient" | "cg_solve"
            | "least_squares" | "lstsq"
            // ── integration extras ───────────────────────────────
            | "romberg" | "romberg_int"
            | "gauss_legendre_quad" | "glquad" | "gl_quad"
            | "monte_carlo_integrate" | "mc_int"
            | "adaptive_simpson" | "asimp"
            // ── LA extras ────────────────────────────────────────
            | "lu_decompose" | "ludec"
            | "qr_decompose" | "qrdec"
            | "householder_reflector" | "householder"
            | "givens_rotation" | "givens"
            | "forward_substitute" | "fwdsub"
            | "back_substitute" | "backsub"
            | "hessenberg_reduce" | "hessen"
            // ── polynomial helpers ───────────────────────────────
            | "poly_derivative" | "polyder"
            | "poly_integrate" | "polyint"
            | "poly_compose" | "poly_eval_horner" | "horner"
            | "pade_approximant" | "pade"
            // ── quaternions ──────────────────────────────────────
            | "quat_mul" | "quat_conj" | "quat_norm" | "quat_inv"
            | "quat_from_axis_angle" | "axis_angle_to_quat"
            | "quat_to_axis_angle"
            | "quat_to_matrix" | "quat_from_matrix" | "matrix_to_quat"
            | "quat_slerp" | "slerp"
            | "euler_zyx_to_matrix" | "matrix_to_euler_zyx"
            | "rotate_3d_vec"
            // ── information theory ───────────────────────────────
            | "kl_divergence" | "kl_div"
            | "js_divergence" | "js_div"
            | "mutual_information" | "mi"
            | "cross_entropy_arr" | "cross_entropy_dist"
            | "renyi_entropy" | "tsallis_entropy"
            // ── quantum ──────────────────────────────────────────
            | "pauli_x" | "pauli_y" | "pauli_z"
            | "pauli_id" | "pauli_i" | "pauli_identity"
            | "ket_bra" | "density_matrix" | "expectation_value" | "expval"
            | "commutator" | "anticommutator"
            | "partial_trace" | "ptrace"
            | "von_neumann_entropy" | "vn_entropy"
            // ── stat mech ────────────────────────────────────────
            | "bose_einstein" | "fermi_dirac"
            | "maxwell_boltzmann_speed" | "mb_speed"
            | "partition_function" | "z_partition"
            | "helmholtz_free_energy" | "free_energy_f"
            | "boltzmann_factor"
            | "einstein_specific_heat" | "einstein_cv"
            // ── optics ───────────────────────────────────────────
            | "fresnel_reflection_te" | "fresnel_reflection_tm"
            | "fresnel_transmission_te" | "fresnel_transmission_tm"
            | "abcd_thin_lens" | "abcd_free_space"
            | "gaussian_beam_q"
            // ── astrodynamics ────────────────────────────────────
            | "kepler_solve"
            | "true_to_eccentric" | "eccentric_to_mean"
            | "julian_date" | "j_date"
            | "jd_to_gregorian" | "jd_to_date"
            | "sidereal_time_gmst" | "gmst"
            | "vis_viva" | "orbital_period_kepler"
            | "orbital_elements_to_state" | "elem_to_state"
            // ── time series ──────────────────────────────────────
            | "kalman_step" | "kalman_filter"
            | "exponential_smoothing" | "exp_smooth"
            | "holt_winters" | "arma_yw_fit" | "ar_yw"
            // ── graph centrality ─────────────────────────────────
            | "pagerank" | "betweenness_centrality" | "closeness_centrality"
            | "eigenvector_centrality" | "degree_centrality" | "triangle_count"
            // ── random samplers ──────────────────────────────────
            | "rgumbel" | "rfrechet" | "rrayleigh"
            | "rlogistic" | "rkumaraswamy" | "rinverse_gamma" | "rinvgamma"
            // ── 2D geometry ──────────────────────────────────────
            | "graham_scan" | "convex_hull_2d"
            | "line_line_intersect_2d" | "ll_intersect_2d"
            | "point_segment_distance" | "p_seg_dist"
            // ── auto-diff ────────────────────────────────────────
            | "forward_diff" | "fdiff"
            | "forward_diff_grad" | "fdiff_grad"
            // ── stat tests ───────────────────────────────────────
            | "bartlett_test" | "levene_test"
            | "fishers_exact_test_2x2" | "fishers_exact"
            | "mcnemar_test"
            | "runs_test" | "wald_wolfowitz"
            | "friedman_test" | "kruskal_wallis_test" | "kruskal"
            | "sign_test"
            | "anderson_darling_normality" | "ad_normality"
            | "jarque_bera_test" | "jb_test"
            | "ljung_box_test" | "ljung_box"
            | "durbin_watson_stat" | "durbin_watson"
            // ── distance metrics ─────────────────────────────────
            | "mahalanobis_distance" | "mahalanobis_dist"
            | "cosine_distance" | "canberra_distance"
            | "bray_curtis_distance" | "bray_curtis"
            | "l1_distance"
            | "chi_squared_distance"
            // ── more distributions ───────────────────────────────
            | "multivariate_normal_pdf" | "mvn_pdf"
            | "multivariate_normal_sample" | "rmvn"
            | "dirichlet_pdf" | "dirichlet_sample" | "rdirichlet"
            | "skellam_pmf"
            | "inverse_gaussian_pdf" | "wald_pdf"
            | "inverse_gaussian_cdf" | "wald_cdf"
            | "inverse_gaussian_sample" | "rwald"
            | "non_central_chi2_pdf" | "ncchi2_pdf"
            // ── matrix functions ─────────────────────────────────
            | "matrix_exp" | "expm" | "matrix_log" | "logm"
            | "matrix_sqrt" | "sqrtm" | "matrix_sin" | "sinm"
            | "matrix_cos" | "cosm"
            // ── adaptive ODE ─────────────────────────────────────
            | "rk45_dormand_prince" | "rk45" | "dopri5"
            | "midpoint_step" | "ode_midpoint"
            | "heun_step" | "ode_heun"
            | "verlet_step" | "ode_verlet"
            // ── GLM ──────────────────────────────────────────────
            | "logistic_regression" | "logit_fit"
            | "poisson_regression"
            | "ridge_regression" | "ridge"
            | "lasso_coord" | "lasso"
            // ── bootstrap/resampling ─────────────────────────────
            | "bootstrap_mean_ci" | "boot_mean_ci"
            | "jackknife_estimate" | "jackknife"
            | "permutation_test_diff" | "perm_test_diff"
            // ── time series extras ───────────────────────────────
            | "acf_at_lag" | "diff_op" | "lag_op"
            | "decompose_classical" | "decompose_ts"
            // ── combinatorial generators ─────────────────────────
            | "combinations_list" | "permutations_list"
            | "cyclic_permutations" | "subsets_of_size"
            // ── DP utilities ─────────────────────────────────────
            | "longest_increasing_subseq" | "lis"
            | "knapsack_01" | "knapsack"
            | "subset_sum_target" | "subset_sum"
            | "coin_change_min" | "coin_change_minimum"
            | "edit_distance_levenshtein" | "edit_distance"
            // ── ML metrics ───────────────────────────────────────
            | "one_hot_encode" | "onehot" | "label_encode"
            | "categorical_cross_entropy" | "cce"
            | "classification_metrics" | "binary_metrics"
            | "roc_auc" | "auroc"
            // ── DSP / image filters ──────────────────────────────
            | "gaussian_blur_kernel" | "sobel_x" | "sobel_y"
            | "prewitt_x" | "prewitt_y"
            | "laplacian_of_gaussian" | "log_kernel"
            // ── stochastic processes ─────────────────────────────
            | "brownian_path" | "wiener_path"
            | "geometric_brownian_path" | "gbm_path"
            | "poisson_process" | "random_walk_1d"
            // ── compression / info ───────────────────────────────
            | "lempel_ziv_complexity" | "lz_complexity"
            | "huffman_code_lengths" | "huffman"
            | "shannon_entropy_rate" | "block_entropy_rate"
            // ── physics / quantum ────────────────────────────────
            | "planck_blackbody" | "blackbody"
            | "rayleigh_jeans" | "compton_shift"
            | "rydberg_energy"
            | "hydrogen_radial_wavefunction" | "h_rad_psi"
            // ── number theory / algebra ──────────────────────────
            | "integer_log" | "ilog"
            | "aks_primality" | "aks"
            | "elliptic_curve_add" | "ec_add"
            | "berlekamp_massey" | "bm_lfsr"
            | "bezout_coefficients" | "bezout" | "extended_euclid"
            // ── CAS-lite ─────────────────────────────────────────
            | "factor_quadratic" | "complete_square"
            | "partial_fraction_simple" | "partial_fraction"
            // ── more quadrature ──────────────────────────────────
            | "gauss_chebyshev_quad" | "gc_quad"
            | "gauss_hermite_quad" | "gh_quad"
            | "gauss_laguerre_quad" | "glag_quad"
            | "clenshaw_curtis_quad" | "cc_quad"
            | "tanh_sinh_quad" | "ts_quad"
            | "gauss_legendre_2d" | "gl_2d"
            | "monte_carlo_2d" | "mc_2d"
            // ── more optimization ────────────────────────────────
            | "simulated_annealing" | "sa_min"
            | "simplex_lp" | "lp_simplex"
            | "particle_swarm" | "pso_min"
            // ── distributions ────────────────────────────────────
            | "gev_pdf" | "gev_cdf" | "gev_sample" | "rgev"
            | "gen_pareto_pdf" | "gen_pareto_cdf"
            | "gen_pareto_sample" | "rgenpareto"
            | "skew_normal_pdf" | "skew_normal_cdf"
            | "mixture_normal_pdf"
            | "categorical_sample" | "rcat"
            | "multinomial_pmf" | "multinomial_sample" | "rmultinom"
            | "truncated_normal_pdf"
            | "truncated_normal_sample" | "rtnorm"
            // ── clustering ───────────────────────────────────────
            | "dbscan" | "gmm_em_1d" | "gmm_1d"
            | "silhouette_score"
            | "davies_bouldin_index" | "db_index"
            | "calinski_harabasz_index" | "ch_index"
            | "mds_2d" | "pcoa_2d" | "mean_shift"
            // ── NN primitives ────────────────────────────────────
            | "batch_norm" | "layer_norm"
            | "dropout_mask"
            | "max_pool_1d" | "avg_pool_1d"
            | "attention_softmax" | "positional_encoding"
            | "glorot_init" | "xavier_init"
            | "he_init" | "kaiming_init"
            | "adam_step" | "rmsprop_step"
            // ── time series ──────────────────────────────────────
            | "ewma" | "ccf" | "periodogram"
            | "welch_psd" | "welch"
            | "lag_features"
            // ── image processing ─────────────────────────────────
            | "median_filter_2d"
            | "threshold_otsu" | "otsu"
            | "histogram_equalize" | "hist_eq"
            | "erode_2d" | "dilate_2d"
            // ── losses ───────────────────────────────────────────
            | "mse_loss" | "mae_loss" | "huber_loss"
            // ── spatial ──────────────────────────────────────────
            | "vincenty_distance" | "vincenty"
            | "mercator_project"
            | "destination_from_bearing" | "dest_bearing"
            // ── integer sequences ────────────────────────────────
            | "recaman" | "recaman_seq"
            | "sylvester" | "sylvester_seq"
            | "happy_q" | "is_happy"
            | "amicable_pair_q"
            | "aliquot_sequence"
            | "magic_constant"
            // ── graph metrics ────────────────────────────────────
            | "clustering_coefficient_local" | "cc_local"
            | "clustering_coefficient_global" | "cc_global"
            | "assortativity" | "common_neighbors" | "jaccard_neighbors"
            | "adamic_adar"
            | "preferential_attachment_score" | "pa_score"
            // ── 3D geometry ──────────────────────────────────────
            | "triangle_3d_normal" | "triangle_3d_area"
            | "tetrahedron_volume"
            | "plane_from_3_points" | "plane_from_pts"
            | "point_to_plane_distance" | "pt_plane_dist"
            | "ray_triangle_intersect" | "moller_trumbore"
            | "ray_sphere_intersect" | "aabb_overlap"
            // ── iterative solvers ────────────────────────────────
            | "gauss_seidel"
            | "jacobi_iteration" | "jacobi_solve"
            | "sor_solve" | "sor"
            | "thomas_tridiag_solve" | "thomas"
            | "richardson_extrapolation" | "richardson"
            | "finite_difference_5pt" | "fd5pt"
            // ── crypto / algebra ─────────────────────────────────
            | "tonelli_shanks_sqrt" | "tonelli_shanks"
            | "baby_step_giant_step" | "bsgs"
            | "pollard_rho_factor" | "pollard_rho"
            | "modular_lcm" | "mlcm"
            | "crt_general" | "crt_arbitrary"
            // ── physics / chemistry ──────────────────────────────
            | "van_der_waals_p" | "vdw_pressure"
            | "nernst_equation" | "nernst"
            | "arrhenius_rate" | "arrhenius"
            | "reduced_mass"
            | "ph_to_concentration" | "ph_to_h"
            // ── MCMC / SDE / HMM ─────────────────────────────────
            | "metropolis_hastings" | "mh_sampler"
            | "gibbs_sampler_step" | "gibbs_step"
            | "euler_maruyama" | "em_sde"
            | "milstein" | "milstein_sde"
            | "ornstein_uhlenbeck_path" | "ou_path"
            | "hmm_forward" | "hmm_viterbi" | "hmm_backward"
            // ── survival / alignment ─────────────────────────────
            | "kaplan_meier" | "km_estimator" | "log_rank_test"
            | "needleman_wunsch" | "nw_align"
            | "smith_waterman" | "sw_align"
            // ── chemistry ────────────────────────────────────────
            | "gibbs_free_energy" | "delta_g"
            | "henderson_hasselbalch" | "hh_eq"
            | "radioactive_decay"
            | "half_life_to_constant" | "hl_to_lambda"
            // ── control theory ───────────────────────────────────
            | "pid_step"
            | "transfer_function_eval" | "tf_eval"
            | "bode_magnitude_db" | "bode_mag_db"
            | "bode_phase_deg"
            | "lqr_2x2"
            // ── game theory ──────────────────────────────────────
            | "nash_eq_2x2" | "nash_2x2"
            | "shapley_value" | "expected_utility"
            // ── operations research ──────────────────────────────
            | "hungarian_assignment" | "hungarian"
            | "tsp_nearest_neighbor" | "tsp_nn"
            | "vertex_cover_2approx" | "vc_2approx"
            // ── PDE ──────────────────────────────────────────────
            | "heat_eq_1d" | "wave_eq_1d"
            | "laplace_2d_jacobi" | "laplace_jacobi"
            // ── Bayesian conjugate ───────────────────────────────
            | "beta_binomial_update"
            | "normal_normal_update"
            | "gamma_poisson_update"
            | "dirichlet_multinomial_update"
            // ── quantum gates ────────────────────────────────────
            | "hadamard_gate" | "h_gate"
            | "cnot_gate" | "cx_gate"
            | "swap_gate" | "cz_gate"
            | "qft_matrix" | "phase_gate"
            | "s_gate" | "t_gate"
            // ── splines ──────────────────────────────────────────
            | "bezier_eval"
            | "catmull_rom_eval" | "cmr_eval"
            | "cubic_hermite_eval" | "ch_eval"
            | "bspline_basis" | "nik_basis"
            // ── music ────────────────────────────────────────────
            | "freq_to_midi" | "midi_to_freq"
            | "equal_temperament_freq"
            | "cents_difference" | "cents_diff"
            // ── astronomy ────────────────────────────────────────
            | "redshift_z" | "hubble_distance" | "luminosity_distance"
            // ── fluid dynamics ───────────────────────────────────
            | "reynolds_number" | "mach_number"
            | "prandtl_number" | "bernoulli_velocity"
            // ── distributions ────────────────────────────────────
            | "negative_binomial_pmf" | "nb_pmf"
            | "hypergeometric_pmf"
            | "beta_binomial_pmf" | "bb_pmf"
            | "von_mises_pdf" | "vmf_pdf"
            // ── random graphs ────────────────────────────────────
            | "erdos_renyi_random" | "erdos_renyi"
            | "barabasi_albert_random" | "barabasi_albert"
            | "watts_strogatz_random" | "watts_strogatz"
            // ── color science ────────────────────────────────────
            | "rgb_to_lab" | "lab_to_rgb"
            | "kelvin_to_rgb" | "color_temp_rgb"
            // ── integer sequences ────────────────────────────────
            | "bell_triangle" | "surjection_count"
            | "distinct_partition_count" | "q_partition"
            | "fibonacci_q" | "is_fib_number"
            // ── stats / divergences / distribs / physics / astro / chem ──
            | "bonferroni_correction" | "bonferroni"
            | "benjamini_hochberg" | "bh_fdr"
            | "tukey_hsd"
            | "hellinger_distance"
            | "wasserstein_1d" | "earth_movers_1d"
            | "chi_squared_divergence"
            | "beta_geometric_pmf"
            | "generalized_gamma_pdf" | "gengamma_pdf"
            | "zip_pmf" | "zero_inflated_poisson_pmf"
            | "stefan_boltzmann_luminosity" | "stellar_luminosity"
            | "photon_momentum" | "photon_energy_ev"
            | "dipole_radiation_power" | "larmor_power"
            | "parallax_to_distance" | "hawking_temperature"
            | "roche_limit" | "apparent_magnitude" | "distance_modulus"
            | "beer_lambert" | "absorbance"
            | "rate_law_n"
            | "freezing_point_depression" | "fpd"
            | "mixed_nash_2x2" | "minimax_2x2"
            // ── graphics / DSP / image / clustering / combinatorics / NT ─
            | "barycentric_coords_2d" | "barycentric_2d"
            | "bresenham_line" | "bilinear_interp_2d"
            | "point_in_polygon_2d"
            | "hilbert_transform" | "cepstrum"
            | "butterworth_lowpass_coeffs" | "butter_lp"
            | "savitzky_golay_coeffs" | "sg_coeffs"
            | "savitzky_golay_filter" | "sg_filter"
            | "canny_edge_intensity" | "canny_intensity"
            | "bilateral_filter_basic" | "bilateral_filter"
            | "kmeans_pp_init" | "kpp_init"
            | "elbow_score" | "wcss"
            | "young_tableaux_count" | "syt_count"
            | "euler_alt_permutation" | "euler_zigzag"
            | "genocchi_number" | "lattice_paths_count"
            | "tetration"
            | "ackermann_limited" | "ackermann"
            | "perfect_power_q" | "b_smooth_q"
            // ── networks / crypto / quantum / geom / TS ──────────
            | "k_core"
            | "rich_club_coefficient" | "rich_club"
            | "rsa_basic_encrypt" | "rsa_enc_int"
            | "rsa_basic_decrypt" | "rsa_dec_int"
            | "dh_shared_secret"
            | "bell_state_phi_plus" | "bell_phi_plus"
            | "bell_state_psi_minus" | "bell_psi_minus"
            | "density_matrix_purity" | "rho_purity"
            | "concurrence_2qubit"
            | "point_in_circle"
            | "circle_circle_intersect_2d"
            | "polygon_centroid"
            | "sutherland_hodgman_clip" | "sh_clip"
            | "kalman_rts_smoother" | "rts_smoother"
            // ── bioinformatics ───────────────────────────────────
            | "gc_content" | "codon_to_aa"
            | "reverse_complement_dna" | "rev_comp_dna"
            | "hamming_dna"
            | "blosum62_pair_score" | "blosum62"
            | "kmer_count"
            // ── geographic ───────────────────────────────────────
            | "great_circle_bearing" | "gc_bearing"
            | "midpoint_lat_lon" | "mid_geo"
            | "utm_zone_for"
            | "area_polygon_lat_lon" | "geo_polygon_area"
            // ── finance ──────────────────────────────────────────
            | "crr_binomial_option" | "crr_option"
            | "bond_price_clean"
            | "bond_yield_to_maturity" | "bond_ytm"
            | "modified_duration_bond"
            | "convexity_bond" | "bond_convexity"
            // ── image quality ────────────────────────────────────
            | "ssim" | "psnr" | "mssim"
            // ── acoustics ────────────────────────────────────────
            | "db_spl_from_pa" | "db_spl"
            | "a_weighting_factor" | "a_weight"
            | "octave_band_center" | "octave_center"
            | "semitone_ratio"
            // ── genetics ─────────────────────────────────────────
            | "hardy_weinberg"
            | "expected_heterozygosity" | "het_e"
            | "fst_simple"
            | "allele_frequencies"
            // ── epidemiology ─────────────────────────────────────
            | "sir_step" | "sir_r0" | "doubling_time"
            // ── economics ────────────────────────────────────────
            | "theil_index"
            | "herfindahl_hirschman" | "hhi"
            | "atkinson_index"
            | "lorenz_curve_points"
            // ── APL/J primitives ─────────────────────────────────
            | "iota_range" | "iota"
            | "reshape_array" | "reshape"
            | "grade_up" | "grade_asc"
            | "grade_down" | "grade_desc"
            // ── plasma physics ───────────────────────────────────
            | "plasma_frequency" | "omega_p"
            | "debye_length" | "lambda_d"
            | "cyclotron_frequency" | "omega_c"
            | "larmor_radius" | "gyroradius"
            // ── string similarity ────────────────────────────────
            | "jaro_winkler_similarity" | "jaro_winkler"
            | "metaphone_simple"
            // ── rating systems ───────────────────────────────────
            | "elo_rating_update" | "elo"
            | "glicko_rating_update" | "glicko"
            | "dice_sum_pmf"
            // ── effect sizes ─────────────────────────────────────
            | "cohens_d" | "effect_size_d"
            | "cliff_delta"
            | "vargha_delaney_a12" | "a12"
            // ── control transient ────────────────────────────────
            | "step_response_2nd_order" | "step_2nd"
            | "overshoot_2nd_order" | "overshoot_pct"
            // ── matrix norms ─────────────────────────────────────
            | "frobenius_norm"
            | "spectral_norm" | "operator_norm_2"
            | "trace_matrix" | "tr_mat"
            // ── networks ─────────────────────────────────────────
            | "homophily_index" | "homophily"
            | "dyad_census" | "triad_census"
            // ── misc ─────────────────────────────────────────────
            | "sigmoid_inverse" | "logit"
            // ── list / string / date / color / music / astro / perm / linguistics / regression / combinatorics / PRNG ──
            | "partition_at" | "drop_at" | "insert_at_idx"
            | "replace_at_index" | "set_at"
            | "swap_indices" | "nth_largest" | "nth_smallest"
            | "position_of_all_matching" | "positions_of_all"
            | "string_take_first" | "string_take_last"
            | "string_drop_first" | "string_drop_last"
            | "pluralize_simple"
            | "singularize_simple" | "singularize"
            | "capitalize_words" | "title_words"
            | "format_table_simple" | "ascii_table"
            | "days_between" | "weeks_between"
            | "months_between" | "years_between"
            | "first_of_month" | "last_of_month"
            | "day_of_week_iso" | "iso_dow"
            | "easter_sunday" | "chinese_zodiac"
            | "iso_week_number" | "iso_week"
            | "relative_luminance" | "wcag_luminance"
            | "contrast_ratio_wcag" | "wcag_contrast"
            | "delta_e_76" | "delta_e"
            | "color_blend_t" | "lerp_color"
            | "chord_to_freqs" | "scale_to_intervals"
            | "interval_semitones"
            | "transpose_freq_semitones" | "transpose_semi"
            | "bpm_to_period" | "midi_to_pitch_class"
            | "key_signature_for" | "circle_of_fifths_step"
            | "moon_phase" | "equation_of_time"
            | "solar_declination" | "sidereal_day_period" | "ecliptic_obliquity"
            | "permutation_order"
            | "permutation_parity" | "perm_sign"
            | "identity_permutation"
            | "permutation_compose" | "perm_mul"
            | "flesch_reading_ease" | "flesch_kincaid_grade"
            | "gunning_fog"
            | "automated_readability_index" | "ari"
            | "lix"
            | "adjusted_r_squared" | "adj_r2"
            | "aic" | "bic"
            | "residuals_compute" | "compute_residuals"
            | "composition_count" | "weak_composition_count"
            | "necklace_count" | "bracelet_count"
            | "multiset_permutations_count" | "multinomial_count"
            | "pearson_hash_byte" | "pearson_hash"
            | "xorshift32_step" | "lcg_next_u32"
            | "fisher_yates_shuffle"
            // ── ──────────────────────────────────────────────────
            | "tetrahedral_number" | "square_pyramidal_number"
            | "octahedral_number" | "pentagonal_pyramidal_number"
            | "cake_number" | "cuban_number" | "centered_hexagonal_number"
            | "carmichael_q" | "is_carmichael"
            | "sphenic_q" | "is_sphenic"
            | "seven_smooth_q" | "is_7_smooth"
            | "cartesian_product_n" | "cart_n"
            | "multiset_union" | "multiset_intersection" | "multiset_difference"
            | "polynomial_roots_dk" | "durand_kerner"
            | "lin_bairstow_step" | "bairstow"
            | "heap_sift_down"
            | "fenwick_build" | "bit_build"
            | "fenwick_query" | "bit_query"
            | "segment_tree_sum" | "seg_sum"
            | "kmp_failure" | "kmp"
            | "z_array" | "z_func"
            | "suffix_array_naive"
            | "manacher_radii" | "manacher"
            | "rabin_karp_hash" | "lcp_array"
            | "regex_escape_simple"
            | "horspool_search" | "bm_horspool"
            | "lpt_schedule" | "lpt"
            | "johnsons_rule" | "johnson_2m"
            | "bit_reverse_32" | "bit_reverse"
            | "bin_to_gray" | "gray_to_bin"
            | "swap_bits_pos" | "swap_bits"
            | "hamming_weight" | "popcnt"
            | "hamming_distance_int" | "hamdist_int"
            | "internal_rate_of_return"
            | "modified_irr" | "mirr"
            | "payback_period_simple" | "payback_simple"
            | "rfc3339_format" | "rfc3339"
            | "rfc3339_parse"
            | "iso_ordinal_date" | "ordinal_date"
            // ── ──────────────────────────────────────────────────
            | "lazy_caterer" | "central_polygonal"
            | "centered_square" | "centered_triangular" | "centered_pentagonal"
            | "star_number" | "dodecahedral_number" | "icosahedral_number"
            | "pronic_number" | "squared_triangular"
            | "woodall_number" | "cullen_number"
            | "repunit" | "repdigit" | "kaprekar_routine_step"
            | "smith_q"
            | "keith_q" | "is_keith"
            | "armstrong_q" | "is_armstrong"
            | "fnv1a_hash" | "djb2_hash"
            | "jenkins_one_at_a_time" | "jenkins_oat"
            | "murmurhash3_x32"
            | "adler32_hash" | "crc16_ccitt"
            | "vec_dot"
            | "l1_norm" | "l2_norm" | "vec_l2"
            | "linf_norm" | "max_norm" | "lp_norm"
            | "unit_vector"
            | "vector_project" | "proj" | "vector_reject"
            | "orthogonalize_vectors" | "gram_schmidt"
            | "outer_product" | "vec_outer"
            | "matrix_diagonal" | "mdiagvec"
            | "matrix_anti_diagonal"
            | "matrix_symmetric_q" | "matrix_orthogonal_q"
            | "geometric_mean_arr" | "harmonic_mean_arr"
            | "quadratic_mean_arr" | "lehmer_mean"
            | "running_mean" | "running_variance"
            | "outlier_iqr_q" | "z_score_robust"
            | "geometric_sequence" | "arithmetic_sequence"
            | "log_sum_exp" | "lse"
            | "log_sigmoid" | "log1p_exp"
            | "string_chars"
            | "string_words_count" | "word_count_simple"
            | "string_lines_count" | "line_count_simple"
            | "string_intersperse" | "string_replicate"
            | "string_uniq_chars" | "string_letter_frequency"
            | "anagram_q" | "is_anagram_q"
            | "string_take_while" | "string_drop_while"
            | "string_split_at_first" | "string_partition_at_word"
            // ── ──────────────────────────────────────────────────
 | "relativistic_kinetic"
            | "lorentz_factor_v" | "doppler_relativistic"
            | "drag_force_quadratic" | "terminal_velocity"
            | "carnot_efficiency" | "otto_efficiency"
            | "brayton_efficiency" | "diesel_efficiency"
            | "specific_heat_const_v" | "speed_of_sound_ideal"
            | "kepler_period_au" | "synodic_period"
            | "hill_radius" | "jeans_length"
            | "chandrasekhar_mass" | "eddington_luminosity"
            | "schwarzschild_radius_m" | "gravity_at_radius"
            | "gravitational_pe"
            | "freefall_time" | "pendulum_freq" | "spring_period"
            | "centripetal_accel" | "lens_focal_length"
            | "avogadros_number" | "boltzmann_const"
            | "planck_const_h" | "gas_constant_r"
            | "concentration_dilute" | "partial_pressure"
            | "mole_fraction" | "molarity" | "molality"
            | "normality_chem" | "ionic_strength"
 | "titration_volume"
            | "atomic_radius_pm" | "de_broglie_wavelength_kg"
 | "lotka_volterra_step"
            | "michaelis_menten" | "hill_equation"
            | "lineweaver_burk" | "eadie_hofstee_y"
            | "arrhenius_temp_q10"
            | "body_surface_area_dubois" | "bsa_dubois"
            | "bmr_harris_benedict_male" | "bmr_harris_benedict_female"
            | "max_heart_rate" | "target_heart_rate"
            | "vo2_max_estimate" | "pulse_pressure"
            | "mean_arterial_pressure" | "map_bp"
            | "dew_point_magnus" | "heat_index_celsius"
            | "wind_chill_celsius" | "pressure_altitude_m"
            | "density_altitude_m" | "saturation_vapor_pressure"
            | "humidex" | "utci_simple"
            | "resistance_parallel" | "r_parallel"
            | "resistance_series" | "r_series"
            | "capacitance_parallel" | "c_parallel"
            | "capacitance_series" | "c_series"
            | "inductance_parallel" | "l_parallel"
            | "inductance_series" | "l_series"
            | "voltage_divider" | "current_divider"
            | "lc_resonant" | "q_factor_rlc"
            | "skin_depth" | "wire_resistance"
            | "motor_torque" | "efficiency_ratio"
            | "dB_voltage" | "db_voltage"
            | "dB_power" | "db_power"
            // ── ──────────────────────────────────────────────────
            | "bfs_distances" | "dfs_preorder" | "connected_components"
            | "graph_is_tree" | "graph_density"
            | "graph_average_degree" | "graph_max_degree" | "graph_min_degree"
            | "graph_complement"
            | "in_degree_directed" | "out_degree_directed"
            | "graph_eccentricity_all" | "is_connected"
            | "articulation_points" | "bridges_edges"
            | "eulerian_path_q" | "hamiltonian_brute"
            | "string_to_charcodes" | "charcodes_to_string"
            | "string_xor"
            | "string_camel_to_snake" | "string_snake_to_camel"
            | "string_kebab_to_snake" | "string_snake_to_kebab"
            | "palindromic_q" | "substring_count"
            | "string_truncate_ellipsis" | "string_expand_tabs"
            | "string_normalize_spaces"
 | "days_in_year" | "quarter_of_year"
            | "zeller_day_of_week" | "age_from_birthdate"
            | "business_days_between" | "unix_epoch_to_iso"
            | "loan_payment_pmt" | "loan_balance"
            | "amortization_total_interest"
            | "apr_to_apy" | "apy_to_apr"
            | "compound_interest_periods" | "simple_interest_compute"

            | "perpetuity_value" | "growing_perpetuity"
            | "annuity_present_value" | "annuity_future_value"
            | "capm_expected_return"
            | "treynor_ratio"
            | "jensens_alpha" | "information_ratio"
            | "friction_factor_laminar" | "swamee_jain_factor"
            | "pipe_pressure_drop" | "orifice_velocity"
            | "chezy_velocity" | "manning_velocity"
            | "froude_number" | "weber_number" | "grashof_number"
            | "nusselt_dittus_boelter"
            // ── more extensions ────────────────────────────────────────────
            | "mollweide_project" | "robinson_project" | "sinusoidal_project"
            | "equirectangular_project" | "lambert_azimuthal_project" | "albers_conic_project"
            | "geohash_encode" | "geohash_decode" | "geohash_neighbor" | "geohash_bbox"
            | "gabor_kernel" | "unsharp_mask_kernel" | "emboss_kernel"
            | "box_blur_kernel" | "motion_blur_kernel" | "sharpen_kernel"
            | "edge_detect_kernel" | "sobel_diagonal_kernel" | "haar_2d_step"
            | "db4_coeffs" | "db6_coeffs" | "sym4_coeffs" | "coif1_coeffs"
            | "aes_sbox_byte" | "aes_inv_sbox_byte"
            | "chacha20_qround" | "xtea_round" | "speck_round" | "simon_round"
            | "kepler_hyperbolic" | "hohmann_dv1" | "hohmann_dv2" | "hohmann_total"
            | "bielliptic_total" | "lambert_simple"
            | "horizon_distance" | "solar_zenith_angle" | "air_mass_kasten"
            | "solar_constant" | "julian_centuries_j2000"
            | "mean_solar_longitude" | "mean_solar_anomaly" | "lst_to_solar"
            | "ra_dec_to_az_alt" | "ecliptic_to_equatorial" | "equatorial_to_galactic"
            | "orbital_eccentricity" | "semi_major_axis"
            | "specific_orbital_energy" | "specific_angular_momentum"
            | "toffoli_gate" | "ccx_gate" | "fredkin_gate" | "cswap_gate"
            | "iswap_gate" | "sqrt_swap_gate"
            | "rx_gate" | "ry_gate" | "rz_gate"
            | "ghz_state_n" | "w_state_n"
            | "depolarizing_channel" | "dephasing_channel" | "amplitude_damping_channel"
            | "quantum_fidelity_pure" | "trace_distance"
            | "bell_inequality_chsh" | "pauli_decomposition_2x2"
            | "quantum_relative_entropy" | "qft_4_real"
            | "bwt_encode" | "bwt_decode" | "mtf_encode" | "mtf_decode"

            | "lyndon_factorize" | "christoffel_word" | "sturmian_word"
            | "z_function_alt" | "period_of_string" | "borders_of_string"
            | "thue_morse_string" | "fibonacci_word"
            | "mann_kendall_tau" | "theil_sen_slope" | "hodges_lehmann"
            | "huber_m_estimator" | "winsorized_variance_arr"
            | "bowley_skewness" | "pearson_skewness_2"
            | "concordance_correlation" | "quantile_p"
            | "label_propagation_step" | "modularity_q"
            | "clique_count_3" | "local_efficiency" | "global_efficiency"
            | "diameter_unweighted"
            | "aitken_delta_squared" | "wynn_epsilon"
            | "shanks_transform" | "levin_t_transform"
            | "harmonic_seq_sum" | "alternating_seq_sum"
            // ── more extensions (2) ────────────────────────────────────────
            | "sparse_csr_build" | "sparse_csr_mul_vec" | "sparse_density"
            | "lower_triangular_q" | "upper_triangular_q"
            | "diagonal_dominance_q" | "matrix_zero_q" | "matrix_identity_q"
            | "matrix_random_uniform" | "matrix_random_normal"
            | "andrew_monotone_chain" | "polygon_area_signed"
            | "polygon_convex_q" | "iou_2d_axis_aligned" | "hausdorff_distance_2d"
            | "minkowski_sum_simple" | "circle_3_points"
            | "polygon_winding_number" | "segment_length"
            | "segments_parallel_q" | "segments_perpendicular_q"
            | "burr_xii_pdf" | "burr_xii_cdf" | "dagum_pdf" | "lomax_pdf"
            | "birnbaum_saunders_pdf" | "tukey_lambda_quantile"
            | "half_cauchy_pdf" | "half_logistic_pdf" | "reciprocal_pdf"
            | "levy_pdf" | "voigt_profile_simple"
            | "gompertz_pdf" | "inverse_weibull_pdf"
            | "log_gamma_simple" | "inverse_chi2_pdf"
            | "poly1305_block_step" | "x25519_field_mul" | "curve25519_mul_simple"
            | "secp256k1_y_recover" | "hmac_step_xor"
            | "pkcs7_pad" | "pkcs7_unpad" | "xor_byte_string"
 | "atbash_cipher"
            | "vigenere_encrypt" | "vigenere_decrypt" | "xor_brute_keylen"
            | "arima_diff" | "seasonal_diff"
            | "garch_step" | "egarch_step"
            | "realized_volatility" | "max_drawdown_arr"
            | "calmar_ratio" | "omega_ratio" | "kelly_criterion"
            | "var_historical" | "cvar_historical"
            | "graph_degree_distribution" | "graph_count_edges"
            | "graph_bipartite_match_simple" | "graph_count_triangles"
            | "graph_avg_clustering" | "graph_transitivity"
            | "graph_max_clique_brute" | "graph_independent_set_brute"
            | "graph_count_paths_length_k" | "graph_pagerank_simple"
            // ── integration / ODE / root finding / optimization ─
            | "boole_rule" | "boole_int"
            | "gauss_legendre_5" | "gl5"
            | "gauss_kronrod_15" | "gk15"

            | "midpoint_rule"
            | "adams_bashforth_4" | "ab4"
            | "heun_method" | "rk45_cash_karp" | "rkck"
            | "milne_pc" | "milne"
            | "modified_midpoint_ode" | "modmidpoint"
            | "backward_euler" | "implicit_euler"
            | "crank_nicolson_ode" | "cn_ode"
            | "brent_root" | "brent" | "ridders_root" | "ridders"
            | "steffensen_root" | "steffensen" | "halley_root" | "halley"
            | "householder_root" | "muller_root" | "muller"
            | "regula_falsi" | "false_position"
            | "secant_root" | "secant"
            | "anderson_step" | "aberth_step" | "inverse_quad_interp"
            | "lm_step" | "gradient_descent_step"
 | "nesterov_step" | "adagrad_step"
            | "cg_beta_pr" | "cg_beta_fr" | "bfgs_h_update_1d"
            | "wolfe_strong_q" | "dogleg_step"
            | "nelder_mead_reflect" | "nelder_mead_expand" | "nelder_mead_contract"
            | "sa_accept_prob" | "sa_boltzmann_temp" | "sa_cauchy_temp"
            | "sa_geometric_temp" | "acceptance_target"
            // ── financial pricing models ────────────────────────
            | "bs_call" | "blackscholes_call" | "bs_put" | "blackscholes_put"
 | "bs_theta_call" | "bs_rho_call"
 | "bachelier_call" | "black76_call"
            | "crr_american_call" | "crr_american_put" | "jr_european_call"
            | "trinomial_call" | "heston_price_simple" | "sabr_implied_vol"
            | "merton_jump_call" | "asian_call_mc" | "barrier_up_out_call"
            | "digital_call" | "lookback_call"
            | "macaulay_duration" | "forward_rate"
            | "discount_continuous" | "ytm_newton"
            | "vasicek_bond" | "cir_bond" | "hull_white_drift"
            | "cds_upfront" | "black_karasinski_drift" | "quanto_adjustment"
            | "fx_forward" | "garman_kohlhagen_call" | "margrabe" | "stulz_min_call"
            | "sharpe_annualized"
            | "jensen_alpha" | "modified_sharpe"
            // ── chemistry ───────────────────────────────────────
            | "ph_from_h" | "poh_from_oh" | "pka_from_ka"
 | "henderson_base"
            | "arrhenius_k" | "eyring_k"
            | "first_order_concentration" | "first_order_half_life"
            | "second_order_concentration" | "second_order_half_life"
            | "zero_order_concentration"

            | "ideal_gas_n" | "redlich_kwong_p"
            | "compressibility_z"
            | "kc_from_rates" | "kp_from_kc" | "reaction_quotient" | "rxn_q"
            | "le_chatelier_dir"
            | "dg_from_k" | "k_from_dg" | "vant_hoff" | "clausius_clapeyron" | "antoine_p"
 | "emf_from_half_cells" | "faraday_mass_deposited"
 | "transmittance" | "ksp_from_concs"
 | "debye_huckel"
            | "cp_monatomic_ideal" | "cv_monatomic_ideal"
            | "heat_capacity_q" | "calorimeter_dt" | "enthalpy_reaction"
            | "avogadro_count" | "moles_from_mass"
            | "dilution_v2" | "raoult_law" | "bp_elevation" | "fp_depression"
            | "osmotic_pressure" | "rydberg_lambda" | "bohr_radius_n"
            | "bohr_energy_ev" | "photon_energy_freq" | "photon_energy_lambda"
            | "de_broglie"
            // ── biology / ecology ───────────────────────────────
 | "logistic_growth_step" | "logistic_growth_analytic"
            | "gompertz_growth_step" | "allee_growth_step"
 | "growth_rate_from_ratio"
 | "seir_step" | "seird_step" | "sis_step"
            | "r0_basic" | "rt_effective" | "herd_immunity_threshold" | "generation_time"
 | "inverse_simpson"
            | "pielou_evenness" | "margalef_richness" | "menhinick_richness"
            | "berger_parker" | "sorensen_dice"
            | "rao_quadratic_entropy"
 | "selection_step" | "nei_genetic_distance"
            | "effective_pop_size" | "carrying_capacity_from_data"
            | "petersen_estimator" | "chapman_estimator"
            | "lv_competition_step"
            | "holling_type1" | "holling_type2" | "holling_type3"
            | "leslie_step" | "net_reproductive_rate" | "generation_time_demo"
            | "finite_rate_lambda" | "kleibers_law" | "bergmann_adjust"
            | "q10" | "species_area" | "intrinsic_growth_rate"
            | "macarthur_wilson_immigration" | "macarthur_wilson_extinction"
            | "island_equilibrium"
            // ── EM / optics / relativity ────────────────────────
 | "efield_point" | "epotential_point"
 | "capacitor_charge"
            | "ohm_voltage" | "power_vi" | "power_i2r"

 | "capacitance_parallel_sum"
            | "bfield_wire" | "bfield_solenoid" | "lorentz_force_mag"
 | "faraday_emf"
 | "lc_frequency" | "lc_omega"
            | "rc_tau" | "rl_tau"
            | "poynting_magnitude" | "em_intensity" | "radiation_pressure"
            | "em_wavelength" | "em_frequency"
            | "snell_theta2"
            | "index_from_speed" | "fresnel_reflection_normal"
            | "fresnel_rs" | "fresnel_rp"
            | "lensmaker" | "thin_lens_v" | "mirror_equation_v"
            | "lens_magnification" | "diffraction_grating_angle"
            | "single_slit_min" | "rayleigh_resolution"
            | "lorentz_gamma"
            | "rel_momentum" | "rel_ke" | "rel_total_energy" | "rel_energy_pm"
            | "relativistic_doppler" | "rel_velocity_add"

            | "wave_string_speed" | "sound_solid" | "sound_gas"
            | "doppler_classical" | "standing_wave_fundamental"
            | "open_pipe_harmonic" | "closed_pipe_harmonic"
            | "sound_db"
            | "alfven_speed"
            | "grav_time_dilation" | "grav_redshift"
            // ── graph algorithms ────────────────────────────────
            | "kosaraju_scc" | "bridges"
            | "max_flow_ek" | "min_cut_value" | "hopcroft_karp"

 | "katz_centrality" | "hits_simple"
            | "pagerank_damped" | "cc_count" | "cc_labels"
            | "topological_sort_kahn" | "has_cycle_directed" | "has_cycle_undirected"
 | "diameter_bfs" | "radius_bfs"
            | "num_edges" | "k_coreness"
            | "greedy_coloring" | "chromatic_number_greedy"
            | "sum_degrees" | "avg_degree" | "max_degree"
            | "is_tree" | "girth"
            // ── signal processing ───────────────────────────────
            | "hamming_window" | "hann_window" | "blackman_window"
            | "blackman_harris_window" | "bartlett_window" | "welch_window"
            | "kaiser_window" | "tukey_window" | "gaussian_window"
            | "hilbert_envelope"
            | "biquad_step" | "biquad_lowpass_coeffs" | "biquad_highpass_coeffs"
            | "biquad_bandpass_coeffs" | "biquad_notch_coeffs" | "biquad_allpass_coeffs"
            | "biquad_peak_coeffs" | "biquad_lowshelf_coeffs" | "biquad_highshelf_coeffs"
            | "butterworth_prewarp" | "butterworth_order"
            | "fir_moving_average" | "fir_lowpass_design"
 | "spectrogram_simple"
            | "zero_pad" | "resample_nearest" | "resample_linear" | "quantize"
            | "mu_law_encode" | "mu_law_decode" | "a_law_encode" | "a_law_decode"
            | "chirp_linear"
            // ── cryptography deep ───────────────────────────────
            | "fnv1a_32" | "fnv1a_64" | "sdbm_hash"
            | "siphash24"
            | "pbkdf2_hmac_step" | "scrypt_round" | "bcrypt_cost_iters"
            | "argon2_block_mix" | "hkdf_expand_step"
            | "lfsr_galois_step" | "mt19937_temper" | "xorshift64" | "xorshift32"
            | "pcg32_step" | "lcg_numrec_step" | "splitmix64_step" | "wyhash_mix"

            | "xor_cipher_byte"
            | "railfence_encrypt" | "beaufort" | "affine_encrypt" | "substitution_encrypt"
            | "letter_frequency" | "english_chi2" | "index_of_coincidence" | "kasiski_repeats"
            | "deterministic_prime" | "dh_shared" | "rsa_encrypt_simple"
            | "monobit_test" | "approximate_entropy"
            // ── ML extensions ───────────────────────────────────
            | "gini_impurity" | "entropy_bits" | "information_gain" | "gain_ratio"
            | "nb_gaussian_likelihood" | "nb_bernoulli_likelihood" | "nb_multinomial_log_likelihood"
            | "adaboost_alpha" | "hinge_loss" | "squared_hinge"
            | "logistic_loss"
 | "sigmoid_grad" | "tanh_grad"
 | "relu_grad"
 | "softsign" | "prelu" | "threshold_act"
            | "confusion_counts" | "mcc" | "f_beta" | "specificity"
            | "balanced_accuracy" | "cohen_kappa" | "brier_score" | "log_loss"
            | "tversky" | "mahalanobis_1d"
 | "one_hot" | "topk_indices"
            | "minmax_scale" | "zscore_norm" | "robust_scale"
            // ── geometry / topology ─────────────────────────────
            | "triangle_area_heron" | "triangle_area_pts"
            | "triangle_inradius" | "triangle_circumradius"
            | "regular_ngon_area" | "regular_ngon_inradius" | "regular_ngon_circumradius"
 | "n_ball_volume"
 | "cylinder_surface" | "cone_surface"

            | "ellipsoid_volume" | "ellipsoid_surface_approx"
            | "dist_point_line_2d" | "dist_point_plane_3d" | "closest_pt_segment_2d"
            | "bbox_from_points"
 | "euclidean_distance_nd"
 | "hamming_distance_str"
 | "great_circle_law_of_cos"
            | "initial_bearing" | "midpoint_great_circle"
            | "shoelace_area" | "polygon_is_convex" | "convex_hull_jarvis"
            | "euler_characteristic" | "genus_from_euler"
            | "spherical_triangle_area" | "polygon_with_holes_area" | "picks_theorem"
            | "centroid_nd" | "covariance_matrix_pts" | "simplex_volume_3d"
            // ── special functions extra ─────────────────────────
            | "hyper2f1" | "hyper1f1" | "hyper0f1" | "pochhammer"
            | "mathieu_ce0" | "mathieu_se1" | "parabolic_d0" | "parabolic_d1"
            | "whittaker_m" | "struve_h0" | "struve_h1"
            | "lambert_w0" | "wright_omega"
            | "sinhc" | "cosh_minus1_over_x2"
            | "sine_integral_si" | "cosine_integral_ci" | "exp_integral_e1"
 | "dawson_function" | "owen_t"
            | "spherical_bessel_j0" | "spherical_bessel_j1"
            | "spherical_bessel_y0" | "spherical_bessel_y1"
            | "mod_sph_bessel_i0" | "mod_sph_bessel_i1" | "mod_sph_bessel_k0"
            | "coulomb_f0"
            | "polylog_li2" | "polylog_n"

 | "ti2" | "clausen_cl2"
            | "bose_einstein_g" | "fermi_dirac_int"
            | "theta3" | "theta2"
            | "jacobi_sn_small_q" | "jacobi_cn_small_q" | "jacobi_dn_small_q"
            | "riemann_xi" | "bessel_jn_general" | "bessel_in_general"
            // ── astronomy / music / color / units ───────────────
 | "absolute_magnitude"
            | "pc_to_ly" | "ly_to_pc" | "pc_to_au" | "au_to_m"
            | "solar_mass_to_kg" | "solar_luminosity_to_w"
            | "hubble_distance_mpc" | "comoving_distance_approx" | "critical_density"
            | "et_freq_ratio" | "midi_to_hz" | "hz_to_midi" | "cents_between"
            | "just_intonation_ratio" | "pythagorean_ratio"
            | "beat_frequency" | "bpm_to_spb" | "note_name_to_midi"
 | "rgb_to_yiq" | "rgb_to_yuv601"
            | "srgb_to_xyz" | "xyz_to_lab" | "delta_e_94"


            | "feet_to_meters" | "meters_to_feet"
            | "lb_to_kg" | "kg_to_lb"
            | "mph_to_kmh" | "kmh_to_mph" | "mps_to_kmh" | "kmh_to_mps" | "knots_to_kmh"
 | "atm_to_pa" | "pa_to_atm" | "mmhg_to_pa"
            | "ev_to_joules" | "joules_to_ev" | "btu_to_joules" | "kwh_to_joules"
            | "bpm_to_midi_tick_us" | "iso226_phon_adjustment"
            | "db_to_amp" | "amp_to_db"
            | "roman_encode" | "roman_decode" | "number_to_english"
            // ── cosmology / GR / FLRW ───────────────────────────
            | "hubble_lcdm" | "hubble_time" | "hubble_distance_si" | "critical_density_si"
            | "comoving_distance" | "angular_diameter_distance"
            | "lookback_time" | "age_at_z" | "scale_factor" | "redshift_from_a"
            | "omega_m_at_z" | "lcdm_eos" | "cpl_w" | "deceleration_q"
            | "schwarzschild_radius_kg" | "kerr_ergosphere_eq" | "kerr_horizon"
 | "bh_entropy" | "bh_evaporation_time"
            | "schwarzschild_isco" | "photon_sphere_radius"
            | "tidal_force" | "grav_dilation_factor" | "lense_thirring_omega"
            | "gw_strain_amplitude" | "chirp_mass" | "grav_binding_energy"
            | "roche_limit_rigid" | "roche_limit_fluid"
            | "lagrange_l1" | "sphere_of_influence"
            | "freefall_velocity_schwarzschild" | "einstein_ring_radius"
            | "microlensing_magnification" | "cosmic_distance_modulus_si"
            | "cmb_temperature" | "cmb_temperature_at_z"
 | "stefan_boltzmann_si" | "planck_spectral_radiance"
            | "schwarzschild_g_tt" | "schwarzschild_g_rr" | "kretschmann_schwarzschild"
            | "hill_velocity" | "vacuum_energy_density"
            | "sound_horizon_recomb" | "bao_scale_today" | "sigma8_default"
            | "lensing_convergence" | "sigma_crit"
            | "perihelion_precession" | "shapiro_delay" | "light_deflection_angle"
 | "tov_mass_limit"
            | "main_sequence_lifetime" | "schwarzschild_freefall_time"
            | "friedmann_density_total" | "cosmological_constant"

 | "planck_energy"
            // ── quantum mechanics deep ──────────────────────────
            | "pure_state_density" | "purity"
            | "linear_entropy" | "quantum_mutual_info"
 | "eof_from_concurrence"
            | "bell_state_index" | "chsh_expectation" | "tsirelson_bound"
            | "pauli_real_part" | "pauli_y_imag"
            | "bloch_to_density_real" | "bloch_purity_check"
            | "fidelity_pure_real" | "l1_coherence" | "relative_entropy_coherence"
            | "kraus_apply" | "bit_flip_prob" | "phase_flip_prob"
            | "depolarizing_density_2x2" | "amplitude_damping_excited"
            | "quantum_fisher_info" | "cramer_rao_bound" | "squeezing_db" | "heisenberg_min"
            | "coherent_mean_photons" | "thermal_mean_photons" | "poisson_photon_pmf"
            | "bose_einstein_pmf" | "mandel_q" | "g2_zero"
            | "free_particle_energy" | "infinite_well_energy" | "harmonic_oscillator_energy"
            | "hydrogen_energy_n" | "stark_shift_linear"
            | "zeeman_energy" | "larmor_frequency" | "rabi_frequency"
            | "schrodinger_step_real" | "probability_density" | "state_norm" | "state_normalize"
 | "quantum_variance" | "spin_casimir"
            | "cg_simple" | "wigner_3j_bound" | "qho_ground_state"
            | "tunneling_prob" | "gamow_factor" | "compton_wavelength" | "uncertainty_position"
            | "berry_phase_spin_half" | "zeno_survival" | "decoherence_time"
            | "ramsey_visibility" | "fermi_golden_rule"
            // ── bioinformatics deep ─────────────────────────────
            | "needleman_wunsch_score" | "smith_waterman_score" | "pam250_score"
            | "tanimoto_bits" | "translate_dna" | "transcribe_dna_rna" | "reverse_transcribe"
            | "at_content" | "tm_wallace" | "tm_marmur" | "codon_adaptation_index"
            | "kmer_jaccard" | "sequence_shannon_info" | "pwm_score"
            | "msa_column_entropy" | "seq_logo_information"
 | "damerau_levenshtein" | "lcs_length"
 | "hirschberg_lcs_length" | "common_kmers"
            | "jukes_cantor_distance" | "kimura_2p_distance" | "felsenstein_step"
            | "branch_length_substitutions" | "num_unrooted_trees" | "bayes_posterior"
            | "hw_expected_counts" | "allele_frequency" | "ld_d" | "ld_r_squared"
 | "heterozygosity" | "ne_from_variance"
            | "expected_coverage" | "lander_waterman_gaps"
            | "bh_adjusted_p" | "zscore_count"
 | "go_enrichment_p" | "blosum45_score"
            | "henikoff_weight" | "hamming_protein" | "codon_usage_variance"
            | "dnds_ratio" | "mutation_rate" | "tajimas_d" | "wattersons_theta"
            | "coalescent_expected_time" | "coalescent_tree_length" | "nm_from_fst"
            // ── ODE advanced ────────────────────────────────────
            | "bdf1_step" | "bdf2_step" | "bdf3_step" | "bdf4_step" | "bdf5_step" | "bdf6_step"
            | "ab1_step" | "ab2_step" | "ab3_step"
            | "am2_step" | "am3_step" | "am4_step"
            | "ros2_step" | "imex_euler_step" | "symplectic_euler_step"
            | "leapfrog_step" | "stormer_verlet_step"
            | "rk4_single" | "dopri5_combine" | "rkf45_error"
            | "lobatto_iiia_2" | "lobatto_iiic_3" | "gauss_irk_2_stage" | "magnus_1st"
            | "euler_lte" | "trapezoidal_lte" | "pi_step_size"
            | "stiffness_ratio" | "spectral_radius"
            | "heun_euler_step" | "bogacki_shampine_step" | "verner_8_combine"
            | "rk_combine" | "ab_coeff_sum"
            | "newmark_beta_step" | "wilson_theta_step"
            | "strang_split" | "lie_split"
            | "exp_euler_step" | "etd_rk2" | "dde_euler_step"
            | "em_step" | "milstein_step" | "heun_sde_step" | "stratonovich_correction"
            | "predictor_corrector" | "numerical_jacobian_col"
            | "cn_coefficient" | "imex_theta_split" | "bulirsch_stoer_step"
            | "cfl_number" | "diffusion_stability"
            | "lax_friedrichs_flux" | "lax_wendroff_flux"
            | "van_leer_limiter" | "minmod_limiter" | "superbee_limiter" | "mc_limiter"
            // ── cryptanalysis & number theory deep ──────────────
            | "pollard_p_minus_1" | "fermat_factor"
            | "trial_smallest_factor" | "bsgs_discrete_log"
            | "mertens" | "liouville"
            | "is_b_smooth" | "primorial_n"
            | "pseudoprime_base2" | "strong_pseudoprime"
            | "aks_witness_count" | "qs_relation"
            | "index_calculus_naive" | "lll_2x2_step" | "coppersmith_bound"
            | "shor_period_prob" | "rsa_d_from_e" | "dh_secret"
            | "elgamal_encrypt" | "ecc_point_double" | "continued_fraction_sqrt"
            | "pell_fundamental" | "sum_two_squares" | "class_number_bound"
            | "smith_normal_2x2_step" | "regulator_naive"
            | "power_residue_check" | "wieferich_check" | "wilson_test"
            | "goldbach_pair" | "english_likeness" | "xor_break_singlebyte"
            | "bit_reverse_64"
            | "gf256_multiply" | "hash_combine"
            // ── econometrics ────────────────────────────────────
            | "arch_lm_test" | "breusch_pagan_test" | "white_robust_se"
            | "newey_west_se" | "hansen_j_test" | "gmm_moment_condition"
            | "hausman_test" | "breusch_godfrey_test" | "box_pierce_test"
            | "adf_test_stat" | "pp_test_stat" | "kpss_test_stat"
            | "dickey_fuller_critical" | "engle_granger_step"
            | "johansen_trace_step" | "vecm_alpha_beta"
            | "panel_within_estimator" | "panel_between_estimator"
            | "panel_random_effects" | "arellano_bond_step"
            | "ols_estimator" | "ols_residual_variance" | "ols_r_squared"
            | "ols_adjusted_r2" | "akaike_info_crit" | "bayesian_info_crit"
            | "hannan_quinn_ic" | "f_statistic_pooled" | "breusch_pagan_lm"
            | "ramsey_reset_test" | "chow_test_stat" | "white_test_stat"
            | "goldfeld_quandt" | "wald_test_stat" | "score_test_stat"
            | "likelihood_ratio_test" | "two_sls_iv" | "iv_estimator"
            | "mle_normal_log_lik" | "mle_exponential_log_lik"
            | "mle_poisson_log_lik" | "gmm_moment_function"
            | "pooling_test_stat" | "heteroskedasticity_test"
            | "robust_se_huber_white" | "bootstrap_se_estimate"
            | "heckman_correction" | "tobit_log_likelihood"
            | "probit_log_likelihood" | "logit_log_likelihood"
            | "multinomial_logit_prob" | "ordered_probit_threshold"
            | "panel_var_step" | "impulse_response_step"
            | "variance_decomposition" | "granger_causality_chi2"
            | "cointegration_residual" | "error_correction_step"
            | "random_walk_innovation" | "random_walk_drift_step"
            | "ar_model_likelihood" | "ma_model_likelihood"
            | "arma_model_innovation"
            // ── algebraic topology, knot theory, lie algebras ───
            | "euler_char_complex" | "betti_zero" | "betti_one" | "betti_two"
            | "genus_surface" | "chern_first_2d" | "genus_curve_arith"
            | "genus_curve_geo" | "hodge_diamond_value" | "poincare_duality"
            | "fundamental_group_zn" | "homology_rank" | "cohomology_rank"
            | "homotopy_group_sphere_pi" | "mapping_class_torus"
            | "linking_number_two" | "writhe_polygon" | "torsion_coefficient"
            | "simplex_volume_n" | "simplicial_volume" | "nerve_complex_count"
            | "cech_zero_cohomology" | "de_rham_zero"
            | "poincare_polynomial_eval" | "chromatic_homology_rank"
            | "khovanov_q_grading" | "hochschild_zero" | "cyclic_homology_step"
            | "group_cohomology_dim" | "group_homology_dim"
            | "abelianization_quotient" | "free_group_rank_lower"
            | "nilpotency_class_lower" | "solvable_length_upper"
            | "schreier_index" | "todd_genus_eval" | "hirzebruch_signature"
            | "chern_simons_action" | "gauss_bonnet_total"
            | "seifert_genus_lower" | "alexander_polynomial_at_one"
            | "jones_polynomial_at_minus_one" | "jones_polynomial_at_i"
            | "homfly_evaluation" | "kauffman_bracket_eval"
            | "cabling_pair_signature" | "seifert_form_2x2"
            | "turaev_alexander_step" | "v_polynomial_eval"
            | "polynomial_jones_skein" | "delta_complex_count"
            | "poset_zeta_two" | "mobius_poset_two" | "mobius_function_pair"
            | "mobius_inversion_step" | "incidence_algebra_dim"
            | "quiver_path_count" | "representation_dim_step"
            | "weyl_group_order" | "root_system_count"
            | "cartan_determinant_a2" | "cartan_matrix_b2"
            | "killing_form_su2" | "casimir_eigenvalue_su2"
            | "universal_enveloping_dim" | "verma_character_step"
            | "plethystic_substitution_value" | "schur_polynomial_eval"
            | "hall_inner_product_two" | "plactic_class_size"
            | "robinson_schensted_pair" | "yamanouchi_word_count"
            | "rsk_size" | "character_su2" | "character_sun"
            | "quantum_dimension_su2" | "quantum_dimension_q"
            | "fusion_rule_su2_step" | "modular_data_s_value"
            | "modular_data_t_value" | "verlinde_count_step"
            | "quantum_invariant_eval" | "operad_count_two"
            | "moduli_dimension_curves" | "hodge_polynomial_eval"
            | "mirror_symmetry_check" | "gromov_witten_invariant"
            | "donaldson_invariant" | "seiberg_witten_value"
            | "floer_homology_rank" | "khovanov_rasmussen_s"
            | "ozsvath_szabo_tau" | "heegaard_genus_lower"
            | "fintushel_stern_step" | "bauer_furuta_step"
            | "geometric_intersection_number"
            | "algebraic_intersection_number"
            // ── electrochemistry, batteries, fuel cells ─────────
            | "nernst_potential_full" | "electrode_potential_step"
            | "exchange_current_density" | "butler_volmer_current"
            | "tafel_anodic_current" | "tafel_cathodic_current"
            | "mass_transport_overpotential" | "limiting_current_density"
            | "diffusion_layer_thickness" | "faradaic_efficiency"
            | "coulombic_efficiency_cell" | "energy_efficiency_cell"
            | "voltaic_efficiency" | "charge_capacity_battery"
            | "energy_density_battery" | "power_density_battery"
            | "specific_capacity_active" | "columbic_capacity_lihalfcell"
            | "ragone_point" | "peukert_capacity" | "peukert_exponent_fit"
            | "shepherd_voltage_step" | "nernst_planck_flux"
            | "debye_length_electrolyte" | "debye_huckel_activity"
            | "gouy_chapman_potential" | "stern_layer_capacitance"
            | "double_layer_capacitance" | "helmholtz_capacitance"
            | "zeta_potential_estimate" | "electroosmotic_velocity"
            | "hagen_poiseuille_eo" | "diffuse_layer_thickness"
            | "poisson_boltzmann_step" | "linearized_pb_step"
            | "electrochem_impedance_z" | "randles_circuit_z"
            | "warburg_impedance" | "cole_cole_eis" | "nyquist_phase"
            | "charge_transfer_resistance" | "solution_resistance_estimate"
            | "ionic_conductivity_arrhenius" | "nernst_einstein_diffusivity"
            | "walden_product" | "kohlrausch_law"
            | "onsager_relation_two_species" | "trasatti_voltammetry_charge"
            | "randles_sevcik_peak" | "levich_current_rde"
            | "koutecky_levich_intercept" | "mott_schottky_capacitance"
            | "flat_band_potential" | "schottky_barrier_height"
            | "photocurrent_density" | "quantum_efficiency_photo"
            | "overall_efficiency_pec" | "fuel_cell_polarization"
            | "electrolyzer_voltage" | "faraday_efficiency_h2"
            | "overpotential_oer" | "overpotential_her"
            | "electrocrystallization_step" | "nucleation_rate_constant"
            | "metal_corrosion_rate" | "pourbaix_line_value"
            | "mixed_potential_step" | "electrochemiluminescence_yield"
            | "solid_electrolyte_capacity" | "ionic_liquid_viscosity_step"
            | "lithium_ion_diffusivity" | "soc_estimate_coulomb"
            | "soh_capacity_fade" | "ocv_lithium_ion_step"
            | "state_of_charge_kalman" | "thermal_runaway_threshold"
            | "joule_heating_battery" | "calorimetric_heat_battery"
            | "abuse_test_voltage" | "swelling_strain_step"
            | "sei_resistance_growth" | "binder_content_optimal"
            | "porosity_active_layer" | "tortuosity_estimate_bruggeman"
            | "electrolyte_decomposition_temp" | "gibbs_thomson_undercooling"
            | "nernst_diffusion_layer" | "diff_coeff_aqueous_estimate"
            | "salt_activity_coefficient" | "mean_activity_coeff_pitzer"
            | "osmotic_coefficient_pitzer" | "debye_huckel_screening_factor"
            | "ph_at_isoelectric" | "buffer_capacity_acid_base"
            | "henderson_hasselbalch_solve" | "titration_endpoint_index"
            // ── tensor calculus, GR, differential geometry ──────
            | "tensor_contract_two" | "tensor_outer_two" | "tensor_trace_index"
            | "tensor_symmetrize_two" | "tensor_antisymmetrize_two"
            | "levi_civita_three" | "levi_civita_four"
            | "kronecker_three" | "kronecker_four"
            | "metric_minkowski_eta_step" | "metric_schwarzschild_step"
            | "metric_kerr_step_simple" | "metric_frw_lapse"
            | "christoffel_first_kind_step" | "christoffel_second_kind_step"
            | "riemann_tensor_step_zero" | "riemann_curvature_normal_form"
            | "ricci_tensor_step_zero" | "scalar_curvature_step"
            | "einstein_tensor_step" | "weyl_tensor_step_zero"
            | "schouten_tensor_step" | "geodesic_equation_step_zero"
            | "parallel_transport_step" | "covariant_derivative_step"
            | "christoffel_symbol_normalize" | "ricci_identity_step"
            | "bianchi_first_identity_check" | "bianchi_second_identity_check"
            | "killing_vector_lie_step" | "lie_derivative_scalar_step"
            | "lie_derivative_vector_step" | "exterior_derivative_one_form"
            | "hodge_star_one_form" | "codifferential_step"
            | "laplace_de_rham_step" | "volume_form_riemannian"
            | "hodge_inner_product_one" | "sectional_curvature_two_plane"
            | "gauss_codazzi_step" | "mainardi_codazzi_step"
            | "weingarten_map_step" | "shape_operator_eig"
            | "mean_curvature_step" | "gaussian_curvature_step"
            | "extrinsic_principal_curv" | "intrinsic_principal_curv"
            | "geodesic_curvature_step" | "darboux_frame_step"
            | "fermi_normal_step" | "synge_world_function"
            | "raychaudhuri_step" | "expansion_scalar_step"
            | "shear_tensor_step" | "twist_tensor_step"
            | "optical_scalars_step" | "peeling_step_psi4"
            | "ads_metric_step" | "de_sitter_metric_step"
            | "warped_product_step_zero" | "kaluza_klein_step"
            | "brans_dicke_step" | "horndeski_step"
            | "einstein_dilaton_step" | "gauss_bonnet_term_2d"
            | "chern_pontryagin_4d_step" | "adm_mass_step"
            | "komar_mass_step" | "bondi_mass_step"
            | "brown_york_quasilocal" | "isolated_horizon_charge"
            | "trapped_surface_check" | "apparent_horizon_step"
            | "event_horizon_check" | "cosmological_constant_term"
            | "de_sitter_radius_step" | "anti_de_sitter_radius_step"
            | "penrose_diagram_factor" | "conformal_compactification_step"
            | "schwarzschild_kruskal_step" | "gullstrand_painleve_step"
            | "kerr_newman_charge_term" | "boyer_lindquist_step"
            | "hartle_thorne_metric" | "oppenheimer_volkoff_step"
            | "post_newtonian_step" | "shapiro_delay_step"
            | "mercury_perihelion_advance"
            | "gravitational_wave_quadrupole"
            | "plus_polarization_amp" | "cross_polarization_amp"
            | "chirp_mass_inspiral_step" | "isco_radius_kerr_step"
            | "spin_orbit_coupling_term" | "spin_spin_coupling_term"
            | "hawking_area_increase" | "unruh_temperature_full"
            | "bekenstein_entropy_step" | "holographic_entanglement_step"
            | "ryu_takayanagi_step" | "swampland_distance_check"
            // ── information theory, coding, signal processing ──
            | "conditional_entropy_step" | "joint_entropy_step"
            | "relative_entropy_kl" | "mutual_information_step"
            | "chain_rule_entropy" | "fano_inequality_bound"
            | "data_processing_inequality" | "arithmetic_coding_interval"
            | "range_coding_step" | "golomb_rice_code"
            | "elias_gamma_code" | "elias_delta_code" | "exp_golomb_code"
            | "fibonacci_code" | "shannon_fano_elias_code"
            | "huffman_balanced_step" | "arithmetic_decode_interval"
            | "range_decode_step" | "universal_code_length"
            | "ziv_lempel_estimate" | "lz77_match_length"
            | "lz78_dictionary_growth" | "lzw_step_dict"
            | "ppm_predict_prob" | "deflate_huffman_lit"
            | "brotli_distance_code_count" | "zstd_window_size_log"
            | "mpeg_quant_value" | "jpeg_zig_zag_index"
            | "jpeg_dct_8x8_quant" | "hadamard_walsh_transform_step"
            | "karhunen_loeve_step" | "discrete_haar_step"
            | "db4_wavelet_step" | "biorthogonal_step"
            | "beylkin_wavelet_step" | "coiflet_wavelet_step"
            | "mallat_pyramid_step" | "threshold_soft_value"
            | "threshold_hard_value" | "median_filter_window"
            | "mean_filter_window" | "gaussian_filter_window"
            | "unsharp_mask_step" | "sobel_kernel_value"
            | "prewitt_kernel_value" | "roberts_kernel_value"
            | "laplacian_kernel_value" | "canny_threshold_step"
            | "hough_accumulator_step" | "ransac_iteration_count"
            | "optical_flow_lk_step" | "horn_schunck_step"
            | "kalman_predict_state" | "kalman_update_state"
            | "particle_filter_resample" | "unscented_sigma_point"
            | "ekf_jacobian_step" | "markov_decision_value"
            | "bellman_equation_step" | "q_learning_update"
            | "policy_iteration_step" | "value_iteration_step"
            | "sarsa_update" | "double_q_learning_step"
            | "ucb1_action_value" | "thompson_sample_beta"
            | "boltzmann_softmax_action" | "explore_exploit_epsilon"
            | "montecarlo_returns_step" | "td_zero_update"
            | "td_lambda_update" | "gradient_temporal_diff"
            | "deep_q_target" | "ddpg_critic_loss_step"
            | "ppo_clip_term" | "trpo_kl_constraint"
            | "a3c_advantage_step" | "ppo_advantage_step"
            | "gae_advantage_step" | "generalized_advantage"
            | "information_bottleneck_step" | "free_energy_principle"
            | "fisher_info_metric" | "kullback_jensen_div"
            | "hellinger_distance_step" | "total_variation_distance"
            | "bhattacharyya_coefficient" | "wasserstein_dist_emp"
            | "chisquare_metric" | "hellinger_kernel"
            | "jensen_shannon_div" | "renyi_divergence_step"
            | "amari_alpha_div" | "csiszar_phi_div"
            | "sinkhorn_iteration_step" | "sliced_wasserstein"
            | "gromov_wasserstein_step" | "spectral_signature_match"
            | "mfcc_coeff_step" | "chroma_feature_step"
            // ── combinatorial optimization, scheduling ──────────
            | "tsp_lower_bound_mst" | "tsp_held_karp_step"
            | "christofides_ratio_bound" | "two_opt_swap_delta"
            | "or_opt_delta" | "three_opt_delta" | "lin_kernighan_step"
            | "nearest_neighbor_tour_step" | "greedy_edge_tour"
            | "nearest_insertion_step" | "farthest_insertion_step"
            | "cheapest_insertion_step" | "max_flow_ford_fulkerson_step"
            | "edmonds_karp_step" | "dinic_blocking_flow"
            | "push_relabel_step" | "boykov_kolmogorov_step"
            | "mincut_stoer_wagner" | "gomory_hu_step"
            | "karger_contract_edge" | "karger_min_cut_count"
            | "maximum_bipartite_matching" | "hopcroft_karp_phase"
            | "blossom_match_step" | "weighted_match_kuhn_step"
            | "hungarian_method_step" | "ap_jonker_volgenant_step"
            | "assignment_lower_bound" | "job_shop_makespan_lower"
            | "flow_shop_johnson_step" | "parallel_machine_lpt"
            | "parallel_machine_spt" | "list_scheduling_step"
            | "graham_2approx_bound" | "chc_bound_makespan"
            | "bin_packing_first_fit" | "bin_packing_best_fit"
            | "bin_packing_next_fit" | "bin_packing_lower_bound_l1"
            | "multidim_packing_step" | "knapsack_01_dp_value"
            | "knapsack_unbounded_dp" | "knapsack_fractional_step"
            | "knapsack_branch_bound" | "knapsack_lp_relaxation"
            | "multi_knapsack_step" | "quadratic_assignment_step"
            | "qap_lower_bound" | "graph_coloring_dsatur_step"
            | "graph_coloring_welsh_powell"
            | "graph_coloring_brooks_bound" | "graph_coloring_lp_bound"
            | "fractional_chromatic_lower" | "list_coloring_step"
            | "edge_coloring_vizing_step" | "clique_number_lower"
            | "independence_number_upper" | "vertex_cover_lp_round"
            | "dominating_set_greedy_step" | "dominating_set_lp_bound"
            | "set_cover_greedy_step" | "set_cover_lp_round"
            | "hitting_set_greedy" | "weighted_set_cover_step"
            | "matroid_greedy_step" | "matroid_intersection_step"
            | "submodular_greedy_step" | "submodular_curvature_bound"
            | "nemhauser_wolsey_bound" | "lp_relax_round"
            | "branch_and_bound_step" | "cutting_plane_step"
            | "gomory_cut_step" | "chvatal_gomory_cut"
            | "mixed_integer_round_up" | "mixed_integer_round_down"
            | "sos_constraint_check" | "column_generation_step"
            | "benders_decomposition_step" | "dantzig_wolfe_step"
            | "lagrangian_relax_step" | "lagrangian_dual_step"
            | "subgradient_step_size" | "nonlinear_dual_step"
            | "augmented_lagrangian_step" | "admm_primal_step"
            | "admm_dual_step" | "proximal_gradient_step"
            | "nesterov_accelerate_step" | "fista_step" | "ista_step"
            | "mirror_descent_step" | "frank_wolfe_step"
            | "conditional_gradient_step" | "greedy_set_cover_round"
            | "local_search_swap_step" | "tabu_search_move_score"
            | "simulated_annealing_step" | "genetic_crossover_one_point"
            | "mutation_bit_flip_prob" | "roulette_wheel_select_index"
            // ── climate, fluids, atmospheric ────────────────────
            | "stefan_boltzmann_radiation" | "emissivity_grey_body"
            | "albedo_blackbody_balance" | "solar_constant_at_distance"
            | "total_solar_irradiance_step" | "absorbed_short_wave"
            | "emitted_long_wave" | "clausius_clapeyron_full"
            | "relative_humidity_step" | "dewpoint_temperature_full"
            | "wet_bulb_potential" | "virtual_temperature_full"
            | "density_altitude_full" | "geopotential_height_full"
            | "geometric_height_full" | "adiabatic_lapse_rate_dry"
            | "adiabatic_lapse_rate_moist" | "brunt_vaisala_full"
            | "richardson_number_step" | "gradient_richardson_full"
            | "flux_richardson_full" | "turbulent_kinetic_energy_step"
            | "mixing_length_prandtl" | "monin_obukhov_length"
            | "similarity_function_phi" | "log_law_wind_profile"
            | "power_law_wind_profile" | "ekman_layer_depth"
            | "ekman_pumping_step" | "geostrophic_wind_step"
            | "gradient_wind_step" | "thermal_wind_step"
            | "quasi_geostrophic_omega" | "omega_equation_step"
            | "potential_temperature_step" | "equivalent_potential_temp"
            | "saturation_equivalent_pt" | "ipv_potential_vorticity"
            | "ertel_pv_step" | "absolute_vorticity_step"
            | "relative_vorticity_step" | "divergence_omega_step"
            | "streamfunction_step" | "velocity_potential_step"
            | "helmholtz_decomp_step" | "courant_friedrichs_lewy"
            | "peclet_number_step" | "prandtl_number_step"
            | "reynolds_full_number" | "schmidt_number_step"
            | "sherwood_number_step" | "nusselt_full_number"
            | "grashof_number_step" | "rayleigh_number_step"
            | "weber_number_step" | "froude_number_step"
            | "strouhal_full" | "mach_full_step"
            | "biot_number_step" | "fourier_number_step"
            | "turbulence_intensity_step" | "hurst_exponent_estimate"
            | "detrended_fluct_alpha" | "power_spectrum_slope"
            | "spectral_kappa_minus53" | "batchelor_scale_step"
            | "kolmogorov_microscale" | "taylor_microscale_step"
            | "integral_length_scale" | "turbulent_dissipation_eps"
            | "isotropic_relation_check" | "sst_anomaly_step"
            | "enso_index_step" | "amo_index_step" | "nao_index_step"
            | "soi_oscillation_index" | "pdo_index_step" | "mjo_phase_step"
            | "walker_circulation_step" | "hadley_cell_max_lat"
            | "ferrel_cell_step" | "itcz_position_lat" | "trade_wind_speed"
            | "westerlies_jet_speed" | "polar_vortex_radius"
            | "arctic_oscillation_step" | "indian_monsoon_index"
            | "african_monsoon_index" | "qbo_oscillation_step"
            | "solar_cycle_phase" | "sunspot_relative_number"
            | "geomagnetic_kp_index" | "ozone_dobson_total"
            | "chlorine_radical_decay" | "montreal_protocol_track"
            | "co2_growth_rate_step" | "methane_growth_rate"
            | "aerosol_optical_depth" | "ice_age_milankovitch"
            | "greenhouse_forcing_step"
            // ── game theory, mechanism design, social choice ────
            | "game_two_player_value" | "nash_equilibrium_pair"
            | "mixed_strategy_value" | "zero_sum_minmax"
            | "saddle_point_check" | "correlated_equilibrium_value"
            | "shapley_value_two_step" | "banzhaf_index_two"
            | "nucleolus_lp_step" | "core_membership_check"
            | "imputation_efficient_check" | "imputation_individual_rational"
            | "prisoners_dilemma_payoff" | "matching_pennies_payoff"
            | "chicken_game_payoff" | "stag_hunt_payoff"
            | "battle_sexes_payoff" | "public_goods_game_payoff"
            | "tragedy_commons_metric" | "ultimatum_acceptance_prob"
            | "dictator_game_share" | "trust_game_repayment"
            | "cooperative_game_value" | "characteristic_function"
            | "bargaining_set_check" | "kalai_smorodinsky_step"
            | "nash_bargaining_solution" | "egalitarian_solution"
            | "utilitarian_solution" | "social_welfare_sum"
            | "arrow_impossibility_check" | "gibbard_satterthwaite_check"
            | "borda_count_step" | "condorcet_winner_check"
            | "plurality_winner_step" | "kemeny_score_step"
            | "dodgson_swap_count" | "coombs_runoff_step"
            | "single_transferable_vote" | "range_voting_score"
            | "approval_voting_max" | "schulze_method_step"
            | "copeland_score_step" | "black_method_winner"
            | "median_voter_step" | "hotelling_location_step"
            | "arrow_pareto_check" | "fair_division_envy_free"
            | "proportional_share" | "maximin_share"
            | "egalitarian_split" | "nash_social_welfare"
            | "divisible_goods_proportional" | "indivisible_envy_free_check"
            | "adjusted_winner_pct" | "sealed_bid_first_price"
            | "sealed_bid_second_price" | "english_auction_step"
            | "dutch_auction_step" | "all_pay_auction_step"
            | "vcg_payment_step" | "revenue_equivalence_check"
            | "truthful_mechanism_check" | "incentive_compatibility_check"
            | "mechanism_design_obj" | "double_auction_step"
            | "combinatorial_auction_step" | "posted_price_offer_accept"
            | "matching_market_step" | "deferred_acceptance_step"
            | "boston_mechanism_step" | "top_trading_cycles_step"
            | "school_choice_match" | "roommate_match_step"
            | "network_formation_step" | "coordination_game_payoff"
            | "evolutionary_stable_strategy" | "replicator_dynamics_step"
            | "hawk_dove_payoff" | "fictitious_play_step"
            | "best_response_dynamic" | "quantal_response_logit"
            | "level_k_step" | "cognitive_hierarchy_step"
            | "sequential_eq_check" | "subgame_perfect_eq"
            | "stackelberg_step" | "cournot_quantity_step"
            | "bertrand_price_step" | "hotelling_price_step"
            | "collusion_payoff_step" | "folk_theorem_value"
            | "repeated_game_avg_payoff" | "discount_factor_step"
            | "trigger_strategy_payoff" | "grim_trigger_step"
            | "tit_for_tat_step" | "prisoners_repeated_eq"
            | "mertens_zamir_step" | "ex_post_value_check"
            | "ex_ante_value_check" | "common_knowledge_iterations"
            // ── symbolic CAS, decompositions, projections ───────
            | "cas_simplify_term" | "cas_expand_two_terms"
            | "cas_factor_quadratic" | "cas_partial_fraction_simple"
            | "cas_polynomial_gcd_step" | "cas_polynomial_div_step"
            | "cas_lagrange_interpolate" | "cas_chebyshev_eval"
            | "cas_legendre_eval" | "cas_hermite_eval"
            | "cas_laguerre_eval" | "cas_jacobi_eval"
            | "cas_gegenbauer_eval" | "cas_taylor_coefficient"
            | "cas_padé_diagonal" | "cas_continued_fraction_step"
            | "cas_resultant_two" | "cas_subresultant_two"
            | "cas_groebner_lt_step" | "cas_buchberger_step"
            | "cas_macaulay_matrix_step" | "cas_modular_inverse"
            | "cas_extended_euclid_step" | "cas_smith_normal_step"
            | "cas_hermite_normal_step" | "cas_radical_simplify"
            | "cas_minimal_polynomial" | "cas_gcd_polynomial_step"
            | "cas_resultant_x_y" | "cas_solve_linear"
            | "cas_solve_quadratic" | "cas_solve_cubic"
            | "cas_solve_quartic" | "cas_solve_polynomial_n"
            | "cas_root_isolate_step" | "cas_sturm_sequence_step"
            | "cas_descartes_rule_count" | "cas_companion_matrix_root"
            | "cas_polynomial_roots_kahan"
            | "cas_eigenvalue_inverse_iteration" | "cas_qr_iteration_step"
            | "cas_jacobi_eigen_step" | "cas_lanczos_iteration_step"
            | "cas_arnoldi_iteration_step" | "cas_givens_rotation_apply"
            | "cas_householder_reflection" | "cas_modified_gram_schmidt"
            | "cas_classical_gram_schmidt" | "cas_rank_revealing_qr"
            | "cas_pivoted_lu_step" | "cas_block_lu_step"
            | "cas_cholesky_step" | "cas_modified_cholesky"
            | "cas_ldlt_step" | "cas_bunch_kaufman_step"
            | "cas_woodbury_identity" | "cas_matrix_pencil_step"
            | "cas_generalized_eigen" | "cas_singular_value_step"
            | "cas_truncated_svd_value" | "cas_pseudoinverse_step"
            | "cas_polar_decomposition" | "cas_schur_decomposition_step"
            | "cas_quasi_triangular" | "cas_riccati_continuous_step"
            | "cas_riccati_discrete_step" | "cas_lyapunov_continuous_step"
            | "cas_lyapunov_discrete_step" | "cas_sylvester_equation_step"
            | "cas_kronecker_product_step" | "cas_vec_operator_step"
            | "cas_matrix_function_step" | "cas_matrix_log_step"
            | "cas_matrix_exp_pade" | "cas_matrix_sqrt_step"
            | "cas_drazin_inverse_step" | "cas_moore_penrose_step"
            | "cas_least_squares_solve" | "cas_total_least_squares"
            | "cas_constrained_ls_step" | "cas_truncated_lsq"
            | "cas_regularized_lsq_tikhonov" | "cas_basis_pursuit_step"
            | "cas_lasso_soft_threshold" | "cas_elastic_net_step"
            | "cas_omp_step" | "cas_iht_iteration"
            | "cas_cosamp_step" | "cas_admm_lasso_step"
            | "cas_proximal_l1_step" | "cas_proximal_l2_step"
            | "cas_proximal_l_inf_step" | "cas_indicator_simplex_proj"
            | "cas_proj_l1_ball" | "cas_proj_l2_ball"
            | "cas_proj_box" | "cas_proj_psd_cone"
            | "cas_proj_soc_step" | "cas_proj_exp_cone"
            | "cas_dykstra_step" | "cas_alternating_projection"
            | "cas_polya_enumeration_step" | "cas_burnside_count_step"
            // ── ML primitives — activations, losses, optimizers ─
            | "ml_relu_step" | "ml_leaky_relu_step" | "ml_elu_step"
            | "ml_selu_step" | "ml_gelu_step" | "ml_swish_step"
            | "ml_mish_step" | "ml_softplus_step" | "ml_softsign_step"
            | "ml_hard_sigmoid" | "ml_hard_tanh" | "ml_prelu_step"
            | "ml_celu_step" | "ml_silu_step" | "ml_logsumexp_step"
            | "ml_log_softmax_step" | "ml_log_sigmoid"
            | "ml_glu_step" | "ml_geglu_step" | "ml_swiglu_step"
            | "ml_attention_score_step" | "ml_scaled_dot_product"
            | "ml_multihead_avg" | "ml_softmax_temperature"
            | "ml_dropout_mask_prob" | "ml_layer_norm_step"
            | "ml_batch_norm_step" | "ml_group_norm_step"
            | "ml_rms_norm_step" | "ml_instance_norm_step"
            | "ml_weight_norm_step" | "ml_spectral_norm_step"
            | "ml_l2_normalize_step" | "ml_huber_loss_step"
            | "ml_smooth_l1_loss" | "ml_focal_loss_step"
            | "ml_dice_loss_step" | "ml_iou_loss_step"
            | "ml_giou_loss_step" | "ml_diou_loss_step"
            | "ml_ciou_loss_step" | "ml_contrastive_loss"
            | "ml_triplet_loss_step" | "ml_arcface_loss_step"
            | "ml_center_loss_step" | "ml_kl_divergence_loss"
            | "ml_cross_entropy_loss" | "ml_binary_cross_entropy"
            | "ml_label_smoothing" | "ml_mixup_lambda"
            | "ml_cutmix_box_iou" | "ml_random_erasing_step"
            | "ml_cosine_lr_schedule" | "ml_warmup_lr_step"
            | "ml_step_lr_schedule" | "ml_exponential_lr"
            | "ml_polynomial_lr" | "ml_one_cycle_lr"
            | "ml_inverse_sqrt_lr" | "ml_cyclic_lr_step"
            | "ml_sgd_step" | "ml_momentum_step"
            | "ml_nesterov_momentum" | "ml_adagrad_step"
            | "ml_rmsprop_step" | "ml_adam_step"
            | "ml_adamw_step" | "ml_adamax_step"
            | "ml_nadam_step" | "ml_radam_step"
            | "ml_lookahead_step" | "ml_lamb_step"
            | "ml_lars_step" | "ml_yogi_step"
            | "ml_amsgrad_step" | "ml_adabelief_step"
            | "ml_shampoo_step" | "ml_lion_step"
            | "ml_sophia_step" | "ml_gradient_clip_norm"
            | "ml_gradient_clip_value" | "ml_gradient_accumulate"
            | "ml_gradient_centralize" | "ml_weight_decay_step"
            | "ml_he_init_value" | "ml_xavier_init_value"
            | "ml_glorot_init_value" | "ml_orthogonal_init"
            | "ml_truncnormal_init" | "ml_kaiming_init"
            | "ml_lecun_init_value" | "ml_zero_init"
            | "ml_constant_init" | "ml_uniform_init"
            | "ml_one_hot_index" | "ml_label_to_id"
            | "ml_id_to_label_step" | "ml_token_logit_top_k"
            | "ml_topk_argmax" | "ml_nucleus_sample_p"
            | "ml_temperature_decay" | "ml_repetition_penalty"
            | "ml_eos_logit_boost"
            // ── NLP — ranking, similarity, language models ──────
            | "nlp_bm25_score" | "nlp_tf_idf_step" | "nlp_okapi_score"
            | "nlp_word_freq_value" | "nlp_doc_freq_step"
            | "nlp_inverse_doc_freq" | "nlp_cosine_similarity_two"
            | "nlp_jaccard_similarity_two" | "nlp_overlap_coefficient"
            | "nlp_dice_coefficient_two" | "nlp_simpson_coefficient"
            | "nlp_levenshtein_dist" | "nlp_damerau_levenshtein"
            | "nlp_jaro_distance" | "nlp_jaro_winkler"
            | "nlp_hamming_distance" | "nlp_lcs_length" | "nlp_lcs_ratio"
            | "nlp_meteor_score" | "nlp_bleu_score_n"
            | "nlp_rouge_score_n" | "nlp_chrf_score" | "nlp_ter_score"
            | "nlp_wer_score" | "nlp_cer_score" | "nlp_perplexity_value"
            | "nlp_bits_per_character" | "nlp_char_ngram_count"
            | "nlp_word_ngram_count" | "nlp_skip_gram_count"
            | "nlp_byte_pair_merge_step" | "nlp_wordpiece_score"
            | "nlp_unigram_lm_score" | "nlp_kneser_ney_step"
            | "nlp_witten_bell_step" | "nlp_good_turing_count"
            | "nlp_laplace_smoothing" | "nlp_lidstone_smoothing"
            | "nlp_jelinek_mercer" | "nlp_dirichlet_smoothing"
            | "nlp_query_likelihood_step" | "nlp_kl_lm_div"
            | "nlp_pmi_score" | "nlp_npmi_score"
            | "nlp_chi2_collocation" | "nlp_loglikelihood_collocation"
            | "nlp_t_score_collocation" | "nlp_dunning_log_likelihood"
            | "nlp_lda_alpha_step" | "nlp_lda_beta_step"
            | "nlp_lda_topic_dist" | "nlp_plsa_step"
            | "nlp_word2vec_skipgram_loss" | "nlp_word2vec_cbow_loss"
            | "nlp_glove_loss_step" | "nlp_fasttext_subword_count"
            | "nlp_byte_level_bpe_step" | "nlp_sentencepiece_score"
            | "nlp_unigram_subword_loss" | "nlp_subword_regularization"
            | "nlp_pointwise_attn_score" | "nlp_relative_position_bias"
            | "nlp_alibi_position_bias" | "nlp_rope_rotary_angle"
            | "nlp_rope_apply_step" | "nlp_position_encoding_sin"
            | "nlp_position_encoding_cos" | "nlp_pe_freq_band"
            | "nlp_max_seq_len_check" | "nlp_token_drop_rate"
            | "nlp_byte_frequency" | "nlp_char_frequency"
            | "nlp_punct_ratio" | "nlp_uppercase_ratio"
            | "nlp_digit_ratio" | "nlp_emoji_ratio"
            | "nlp_url_count" | "nlp_email_count" | "nlp_phone_count"
            | "nlp_hashtag_count" | "nlp_mention_count"
            | "nlp_token_overlap_two" | "nlp_word_mover_dist"
            | "nlp_sif_weight_step" | "nlp_doc_embedding_avg"
            | "nlp_attention_pool_step" | "nlp_max_pool_step"
            | "nlp_avg_pool_step" | "nlp_sum_pool_step"
            | "nlp_self_attn_compute_step" | "nlp_cross_attn_compute_step"
            | "nlp_window_attn_step" | "nlp_strided_attn_step"
            | "nlp_block_attn_step" | "nlp_sliding_window_step"
            | "nlp_local_attn_step" | "nlp_dilated_attn_step"
            | "nlp_global_attn_step" | "nlp_sparse_attn_score"
            | "nlp_linformer_step" | "nlp_performer_step"
            | "nlp_reformer_step" | "nlp_longformer_step"
            | "nlp_bigbird_step" | "nlp_routing_attn_step"
            // ── graphics, geometry, ray tracing, BRDF, color ────
            | "gfx_perspective_proj_x" | "gfx_perspective_proj_y"
            | "gfx_orthographic_proj" | "gfx_view_matrix_step"
            | "gfx_lookat_forward" | "gfx_lookat_right" | "gfx_lookat_up"
            | "gfx_quat_to_axis_angle" | "gfx_axis_angle_to_quat"
            | "gfx_quat_slerp_step" | "gfx_quat_nlerp_step"
            | "gfx_quat_dot_two" | "gfx_quat_inverse_step"
            | "gfx_quat_to_euler_pitch" | "gfx_quat_to_euler_yaw"
            | "gfx_quat_to_euler_roll" | "gfx_euler_to_quat_x"
            | "gfx_euler_to_quat_y" | "gfx_euler_to_quat_z"
            | "gfx_euler_to_quat_w" | "gfx_rotation_matrix_xx"
            | "gfx_rotation_matrix_yy" | "gfx_rotation_matrix_zz"
            | "gfx_translation_matrix_step" | "gfx_scale_matrix_step"
            | "gfx_shear_matrix_xy" | "gfx_homogeneous_divide"
            | "gfx_screen_space_x" | "gfx_screen_space_y"
            | "gfx_ndc_to_screen_x" | "gfx_ndc_to_screen_y"
            | "gfx_screen_to_ndc_x" | "gfx_screen_to_ndc_y"
            | "gfx_clip_polygon_step" | "gfx_sutherland_hodgman"
            | "gfx_cohen_sutherland_code" | "gfx_liang_barsky_t"
            | "gfx_bresenham_step_x" | "gfx_bresenham_step_y"
            | "gfx_xiaolin_wu_intensity" | "gfx_aabb_intersect_check"
            | "gfx_obb_overlap_step" | "gfx_sphere_intersect_t"
            | "gfx_ray_triangle_t" | "gfx_ray_plane_t" | "gfx_ray_box_t"
            | "gfx_ray_sphere_t" | "gfx_ray_disk_t"
            | "gfx_ray_cylinder_t" | "gfx_ray_cone_t"
            | "gfx_ray_ellipsoid_t" | "gfx_ray_torus_t_approx"
            | "gfx_barycentric_alpha" | "gfx_barycentric_beta"
            | "gfx_barycentric_gamma" | "gfx_phong_diffuse_step"
            | "gfx_phong_specular_step" | "gfx_phong_ambient_step"
            | "gfx_blinn_specular_step" | "gfx_lambert_term"
            | "gfx_oren_nayar_term" | "gfx_cook_torrance_d_ggx"
            | "gfx_cook_torrance_g_smith" | "gfx_cook_torrance_f_schlick"
            | "gfx_disney_principled_d" | "gfx_microfacet_brdf_step"
            | "gfx_subsurface_scattering_term" | "gfx_translucent_falloff"
            | "gfx_normal_distribution_ggx"
            | "gfx_geometric_attenuation_smith"
            | "gfx_fresnel_dielectric_step" | "gfx_fresnel_conductor_step"
            | "gfx_index_of_refraction" | "gfx_snells_law_angle"
            | "gfx_total_internal_reflection" | "gfx_refract_direction_x"
            | "gfx_reflect_direction_x" | "gfx_environment_map_uv_u"
            | "gfx_environment_map_uv_v" | "gfx_cube_map_face_index"
            | "gfx_octahedral_encode_x" | "gfx_octahedral_encode_y"
            | "gfx_spherical_harmonic_y00" | "gfx_spherical_harmonic_y10"
            | "gfx_spherical_harmonic_y11" | "gfx_spherical_harmonic_y20"
            | "gfx_zonal_harmonic_step" | "gfx_irradiance_sh_eval"
            | "gfx_radiance_sh_eval" | "gfx_skybox_uv_u" | "gfx_skybox_uv_v"
            | "gfx_tonemap_reinhard" | "gfx_tonemap_aces"
            | "gfx_tonemap_uncharted2" | "gfx_tonemap_filmic"
            | "gfx_gamma_correct_step" | "gfx_srgb_to_linear"
            | "gfx_linear_to_srgb" | "gfx_dither_bayer_4x4"
            | "gfx_dither_floyd_steinberg" | "gfx_oklab_l_step"
            | "gfx_oklab_a_step" | "gfx_oklab_b_step"
            | "gfx_oklch_chroma" | "gfx_oklch_hue"
            | "gfx_pcg_hash_step" | "gfx_xorshift_step"
            | "gfx_halton_step" | "gfx_sobol_step"
            | "gfx_van_der_corput" | "gfx_low_discrepancy_step"
            | "gfx_blue_noise_value" | "gfx_perlin_noise_step"
            | "gfx_simplex_noise_step" | "gfx_fbm_noise_step"
            | "gfx_worley_noise_step" | "gfx_voronoi_distance"
            | "gfx_curl_noise_step" | "gfx_gradient_noise_step"
            | "gfx_value_noise_step" | "gfx_signed_distance_box"
            | "gfx_signed_distance_sphere" | "gfx_signed_distance_capsule"
            // ── database internals, distributed systems ─────────
            | "db_b_tree_split" | "db_b_tree_merge"
            | "db_lsm_compaction_step" | "db_skiplist_height_pick"
            | "db_bloom_filter_bit_index" | "db_cuckoo_filter_fingerprint"
            | "db_quotient_filter_canonical" | "db_count_min_sketch_bin"
            | "db_hyperloglog_register_max" | "db_min_hash_value"
            | "db_simhash_bit" | "db_consistent_hash_index"
            | "db_rendezvous_hash_score" | "db_jump_hash_bucket"
            | "db_maglev_hash_step" | "db_lru_cache_eviction_age"
            | "db_lfu_cache_decay" | "db_arc_cache_score"
            | "db_clock_cache_hand" | "db_tinylfu_admit_score"
            | "db_w_tinylfu_freq" | "db_buffer_pool_score"
            | "db_query_plan_cost_step" | "db_join_selectivity_step"
            | "db_index_seek_cost" | "db_seq_scan_cost"
            | "db_index_scan_cost" | "db_sort_cost_estimate"
            | "db_hash_join_cost" | "db_merge_join_cost"
            | "db_nested_loop_cost" | "db_query_cardinality"
            | "db_histogram_bucket_index" | "db_quantile_estimate_p99"
            | "db_t_digest_centroid" | "db_kll_quantile_step"
            | "db_dd_sketch_bin" | "db_reservoir_sample_index"
            | "db_chao_estimator_step" | "db_jaccard_minhash_estimate"
            | "db_distinct_estimate_lpc" | "db_distinct_estimate_hll"
            | "db_throttle_token_step" | "db_leaky_bucket_step"
            | "db_token_bucket_step" | "db_circuit_breaker_step"
            | "db_two_phase_commit_step" | "db_three_phase_commit_step"
            | "db_paxos_propose_id" | "db_raft_term_advance"
            | "db_raft_log_match_check" | "db_zab_epoch_step"
            | "db_chubby_lease_step" | "db_logical_clock_step"
            | "db_lamport_timestamp" | "db_vector_clock_merge"
            | "db_hybrid_logical_clock" | "db_crdt_g_counter_merge"
            | "db_crdt_pn_counter_merge" | "db_crdt_lww_register_merge"
            | "db_crdt_set_or_merge" | "db_consensus_quorum_size"
            | "db_replication_lag_step" | "db_partitions_for_n"
            | "db_consistent_lookup_id" | "db_chord_finger_index"
            | "db_kademlia_xor_distance" | "db_pastry_routing_step"
            | "db_dht_replicate_factor" | "db_partition_failure_check"
            | "db_byzantine_quorum_size" | "db_pbft_view_change"
            | "db_honey_badger_step" | "db_avalanche_query_step"
            | "db_quorum_intersection_check" | "db_anti_entropy_step"
            | "db_merkle_node_hash" | "db_merkle_path_verify"
            | "db_gossip_fanout_step" | "db_anti_entropy_pull_step"
            | "db_split_brain_check" | "db_clock_skew_estimate"
            | "db_freshness_score" | "db_read_repair_step"
            | "db_hinted_handoff_step" | "db_compaction_score"
            | "db_levelled_compaction_step" | "db_size_tiered_compaction"
            | "db_universal_compaction_step" | "db_write_amplification"
            | "db_read_amplification" | "db_space_amplification"
            | "db_block_cache_hit_rate" | "db_page_cache_eviction_age"
            | "db_wal_fsync_cost" | "db_group_commit_count"
            | "db_replica_lag_threshold" | "db_synchronous_commit_check"
            | "db_async_commit_check" | "db_eventual_consistency_check"
            | "db_strong_consistency_check" | "db_linearizability_check"
            | "db_causal_consistency_check"
            // ── networking — TCP, AQM, MIMO, queueing ───────────
            | "net_tcp_cwnd_step" | "net_tcp_ssthresh_update"
            | "net_tcp_reno_step" | "net_tcp_cubic_step"
            | "net_tcp_bbr_step" | "net_tcp_vegas_step"
            | "net_tcp_westwood_step" | "net_tcp_compound_step"
            | "net_tcp_dctcp_step" | "net_tcp_yeah_step"
            | "net_tcp_htcp_step" | "net_tcp_hybla_step"
            | "net_tcp_illinois_step" | "net_tcp_lp_step"
            | "net_tcp_scalable_step" | "net_tcp_veno_step"
            | "net_aiad_step" | "net_aimd_step"
            | "net_miad_step" | "net_mimd_step"
            | "net_aqm_red_drop_prob" | "net_aqm_codel_target"
            | "net_aqm_pie_drop_rate" | "net_aqm_fq_codel_step"
            | "net_aqm_blue_step" | "net_aqm_choke_step"
            | "net_aqm_sfq_step" | "net_aqm_drr_step"
            | "net_aqm_wrr_step" | "net_token_rate_limit"
            | "net_traffic_shaper_step" | "net_priority_queue_index"
            | "net_packet_loss_estimate" | "net_jitter_estimate"
            | "net_latency_avg" | "net_rtt_smoothed"
            | "net_rtt_variation" | "net_rto_compute"
            | "net_bandwidth_delay_product" | "net_path_capacity_kleinrock"
            | "net_loss_rate_to_throughput" | "net_throughput_padhye"
            | "net_throughput_mathis" | "net_throughput_response"
            | "net_router_buffer_size" | "net_drop_tail_check"
            | "net_burst_size_compute" | "net_packet_pacing_step"
            | "net_link_capacity_share" | "net_proportional_fair_share"
            | "net_max_min_fair_step" | "net_alpha_fair_step"
            | "net_kelly_pricing_step" | "net_network_utility_max"
            | "net_lyapunov_drift_plus_penalty" | "net_backpressure_step"
            | "net_max_weight_match" | "net_qcsma_propose"
            | "net_csma_back_off" | "net_alohanet_throughput"
            | "net_slotted_aloha_throughput" | "net_csma_efficiency"
            | "net_token_ring_efficiency" | "net_polling_efficiency"
            | "net_radio_path_loss" | "net_friis_received_power"
            | "net_two_ray_ground_loss" | "net_okumura_hata_loss"
            | "net_log_distance_path" | "net_shadowing_normal"
            | "net_rician_k_factor" | "net_rayleigh_envelope"
            | "net_doppler_shift" | "net_capacity_shannon"
            | "net_mimo_capacity_step" | "net_zero_forcing_beam"
            | "net_mmse_beam_step" | "net_water_filling_power"
            | "net_amc_threshold_index" | "net_harq_combining_gain"
            | "net_turbo_decode_iter" | "net_ldpc_iteration_step"
            | "net_polar_decode_step" | "net_viterbi_step"
            | "net_bcjr_step" | "net_outage_probability"
            | "net_diversity_gain" | "net_array_gain"
            | "net_multiplexing_gain" | "net_coding_gain"
            | "net_pruning_gain" | "net_macro_diversity_step"
            | "net_micro_diversity_step" | "net_handoff_threshold"
            | "net_call_admission_check" | "net_blocking_probability"
            | "net_erlang_b_formula" | "net_erlang_c_formula"
            | "net_engset_formula" | "net_little_law_l"
            | "net_throughput_law" | "net_response_time_law"
            | "net_utilization_law" | "net_forced_flow_law"
            // ── OS internals — schedulers, I/O, memory ──────────
            | "os_priority_aging_step" | "os_mlfq_demote_step"
            | "os_mlfq_promote_step" | "os_round_robin_quantum"
            | "os_completely_fair_vruntime" | "os_lottery_ticket_count"
            | "os_stride_pass_step" | "os_eevdf_eligible"
            | "os_cfs_load_balance_step" | "os_eas_energy_estimate"
            | "os_smt_threading_share" | "os_numa_node_distance"
            | "os_cpu_affinity_score" | "os_thread_migration_cost"
            | "os_load_average_decay" | "os_runqueue_depth"
            | "os_io_scheduler_deadline" | "os_io_scheduler_cfq_step"
            | "os_io_scheduler_noop_step" | "os_io_scheduler_bfq_step"
            | "os_io_scheduler_kyber_step" | "os_io_scheduler_mq_deadline"
            | "os_anticipation_window" | "os_elevator_step"
            | "os_disk_seek_time" | "os_disk_rotational_lat"
            | "os_disk_transfer_time" | "os_pre_fetch_window"
            | "os_buffer_cache_pages" | "os_dirty_page_threshold"
            | "os_writeback_step" | "os_swappiness_factor"
            | "os_kswapd_wake_threshold" | "os_oom_score_step"
            | "os_page_replacement_lru" | "os_page_replacement_clock"
            | "os_page_replacement_2q" | "os_working_set_size"
            | "os_thrashing_threshold" | "os_demand_paging_step"
            | "os_copy_on_write_check" | "os_zero_page_optimization"
            | "os_huge_page_threshold" | "os_transparent_hugepage"
            | "os_kasan_shadow_offset" | "os_kfence_check"
            | "os_kfence_alloc_index" | "os_slub_object_size_round"
            | "os_slab_color_offset" | "os_per_cpu_cache_size"
            | "os_buddy_order_pick" | "os_compact_memory_step"
            | "os_kvm_vmcs_field_offset" | "os_apic_irq_priority"
            | "os_msi_x_vector_count" | "os_iommu_domain_step"
            | "os_pci_bus_address" | "os_acpi_state_transition"
            | "os_cpufreq_governor_step" | "os_intel_pstate_target"
            | "os_amd_pstate_target" | "os_thermal_zone_trip"
            | "os_throttle_temperature" | "os_battery_capacity_pct"
            | "os_powertop_score" | "os_idle_state_select"
            | "os_c_state_residency" | "os_p_state_voltage"
            | "os_dvfs_step" | "os_voltage_scaling_step"
            | "os_frequency_scaling_step" | "os_inotify_event_count"
            | "os_epoll_ctl_count" | "os_io_uring_sqe_count"
            | "os_io_uring_cqe_count" | "os_kqueue_event_count"
            | "os_systemd_journal_size" | "os_dmesg_severity_level"
            | "os_audit_event_priority" | "os_apparmor_profile_active"
            | "os_selinux_context_match" | "os_smack_label_compare"
            | "os_capability_check" | "os_seccomp_filter_step"
            | "os_namespace_isolation" | "os_cgroup_v1_count"
            | "os_cgroup_v2_count" | "os_pid_max_value"
            | "os_thread_max_value" | "os_file_max_value"
            | "os_open_files_count" | "os_socket_max_value"
            | "os_inotify_max_watches" | "os_oom_kill_score"
            | "os_zswap_compress_ratio" | "os_zram_compress_ratio"
            | "os_swap_pressure_score" | "os_pressure_stall_step"
            | "os_psi_avg10_step" | "os_psi_avg60_step"
            | "os_psi_avg300_step" | "os_load_proc_avg"
            | "os_load_user_avg" | "os_load_iowait_avg"
            // ── security — KDFs, MFA, PKI, web sec, TLS ─────────
            | "sec_argon2_memcost" | "sec_argon2_timecost"
            | "sec_argon2_parallelism" | "sec_argon2_block_step"
            | "sec_pbkdf2_iter" | "sec_scrypt_n_param"
            | "sec_scrypt_r_param" | "sec_scrypt_p_param"
            | "sec_balloon_hash_step" | "sec_yescrypt_step"
            | "sec_bcrypt_cost_factor" | "sec_bcrypt_round_step"
            | "sec_password_strength_zxcvbn" | "sec_haveibeenpwned_check"
            | "sec_diceware_word_index" | "sec_xkcd_passphrase_score"
            | "sec_passphrase_entropy" | "sec_chosen_charset_strength"
            | "sec_keystroke_timing_var" | "sec_2fa_totp_window"
            | "sec_totp_drift_check" | "sec_hotp_counter_step"
            | "sec_yubikey_otp_check" | "sec_webauthn_attestation_check"
            | "sec_fido2_assertion_check" | "sec_certificate_chain_depth"
            | "sec_revocation_ocsp_check" | "sec_crl_age_seconds"
            | "sec_pki_path_validate" | "sec_x509_subject_match"
            | "sec_san_match_count" | "sec_basic_constraints_ca"
            | "sec_pinning_compare" | "sec_certificate_transparency"
            | "sec_dane_tlsa_match" | "sec_hpkp_pin_match"
            | "sec_csp_directive_match" | "sec_csrf_token_match"
            | "sec_cors_origin_match" | "sec_xss_filter_score"
            | "sec_html_escape_check" | "sec_url_safe_encode_check"
            | "sec_path_traversal_detect" | "sec_sqli_pattern_score"
            | "sec_xxe_pattern_score" | "sec_xxe_dtd_check"
            | "sec_command_injection_score" | "sec_idor_check"
            | "sec_jwt_alg_safe" | "sec_jwt_kid_match"
            | "sec_jwt_signature_verify" | "sec_oauth2_state_validate"
            | "sec_oauth2_pkce_step" | "sec_oauth_nonce_check"
            | "sec_session_lifetime" | "sec_idle_timeout_step"
            | "sec_login_throttle_step" | "sec_account_lockout_step"
            | "sec_password_history_check" | "sec_complexity_policy_score"
            | "sec_dictionary_attack_check" | "sec_brute_force_attempts"
            | "sec_credential_stuffing_score" | "sec_kerberos_ticket_age"
            | "sec_kerberos_pac_check" | "sec_kerberos_pre_auth"
            | "sec_ldap_bind_step" | "sec_radius_auth_step"
            | "sec_diameter_avp_step" | "sec_saml_assertion_age"
            | "sec_oidc_id_token_age" | "sec_acme_dns_challenge"
            | "sec_dnssec_signature_check" | "sec_spf_pass_check"
            | "sec_dkim_signature_check" | "sec_dmarc_policy_check"
            | "sec_arc_chain_step" | "sec_smtp_ssl_check"
            | "sec_imap_starttls_check" | "sec_pop3_security_step"
            | "sec_tls_alert_severity" | "sec_tls13_handshake_step"
            | "sec_tls12_handshake_step" | "sec_tls11_deprecation_check"
            | "sec_ssl3_disabled_check" | "sec_cipher_suite_strength"
            | "sec_cbc_mac_block_count" | "sec_gcm_iv_unique_check"
            | "sec_chachapoly_nonce_check" | "sec_x25519_clamping_step"
            | "sec_ed25519_signature_step" | "sec_ed448_signature_step"
            | "sec_p384_curve_step" | "sec_secp256k1_step"
            | "sec_blake3_chunk_step" | "sec_keccak_round_step"
            | "sec_sha3_padding_step" | "sec_argon2_state_advance"
            | "sec_chacha20_quarterround" | "sec_aes_round_step"
            | "sec_aes_keyschedule_step" | "sec_des_round_step"
            | "sec_blowfish_round_step" | "sec_serpent_round_step"
            | "sec_twofish_round_step"
            // ── calendrical algorithms ──────────────────────────
            | "fixed_from_gregorian" | "gregorian_from_fixed"
            | "fixed_from_julian" | "julian_from_fixed"
            | "iso_week_date" | "hebrew_leap_year"
            | "hebrew_year_length" | "fixed_from_hebrew"
            | "islamic_leap_year" | "fixed_from_islamic"
            | "persian_arithmetic_leap" | "fixed_from_persian"
            | "coptic_from_fixed" | "ethiopic_from_fixed"
            | "french_revolutionary_leap" | "fixed_from_french"
            | "chinese_year_zodiac" | "chinese_lunation_winter"
            | "hindu_solar_year" | "hindu_lunisolar_month"
            | "maya_long_count_from_fixed" | "mayan_haab_from_fixed"
            | "mayan_tzolkin_from_fixed" | "badi_year_from_fixed"
            | "bahai_from_fixed" | "easter_gregorian_year"
            | "easter_orthodox_year" | "easter_julian_year"
            | "day_of_week_zeller" | "iso_day_number"
            | "weekday_name_short" | "leap_year_gregorian"

            // ── R / SciPy distributions and tests ───────────────
            | "dnorm" | "dt" | "df_dist" | "dchisq"
            | "glm" | "aov" | "shapiro_wilk" | "anderson_darling"
            | "kolmogorov_smirnov" | "spearmanr" | "kendalltau" | "pearsonr"
            | "mannwhitneyu" | "wilcoxon" | "kruskal_h"

            // ── APL/J/K array primitives ────────────────────────
            | "iota_n" | "reduce_axis" | "scan_axis" | "fold_axis"
            | "rotate_axis" | "transpose_axis" | "reshape_dim"
            | "encode_base" | "decode_base" | "nub_list" | "nub_count"
            | "membership_idx" | "deal_n_k" | "roll_n"
            | "permute_idx" | "invert_perm"

            // ── astronomy / astrometry ──────────────────────────
            | "julian_day" | "jd_to_calendar" | "tt_to_tdb"
            | "ra_dec_to_alt_az" | "alt_az_to_ra_dec"
            | "precession_iau2006" | "nutation_iau2000a"
            | "aberration_annual" | "proper_motion_apply"
            | "parallax_correction" | "sun_position_low" | "sun_distance_au"
            | "moon_position_low" | "moon_phase_age" | "lunation_index"
            | "eclipse_magnitude" | "saros_cycle" | "metonic_cycle"
            | "orbit_kepler3" | "orbital_period_au" | "orbit_eccentric_anomaly"
            | "escape_velocity_body" | "hill_sphere_radius" | "tisserand_param"
            | "tle_mean_motion" | "sgp4_propagate_step" | "airy_disk_radius"
            | "rayleigh_criterion" | "strehl_ratio" | "au_to_km"

            // ── sports analytics — ratings & sabermetric ────────
            | "elo_expected" | "elo_update" | "glicko_rating"
            | "trueskill_update" | "trueskill_match_quality"
            | "pythagorean_expectation" | "war_above_replacement"
            | "woba_weight" | "wrc_plus" | "ops_plus" | "era_plus"
            | "fip" | "xfip" | "siera" | "babip" | "wpa"
            | "win_probability" | "leverage_index" | "clutch_score"
            | "shooting_pct" | "save_pct" | "corsi_for" | "fenwick_for"
            | "goals_above_avg" | "tackle_efficiency" | "yards_per_attempt"
            | "qbr_metric" | "epa_per_play"

            // ── Excel/Sheets + bond/loan financial ──────────────
            | "vlookup" | "hlookup" | "xlookup" | "index_match"
            | "indirect" | "choose" | "offset"
            | "sumif" | "countif" | "averageif"
            | "sumifs" | "countifs" | "averageifs"
            | "sumproduct" | "rank_eq" | "rank_avg" | "percentrank"
            | "quartile_inc" | "quartile_exc"
            | "xnpv" | "ppmt" | "ipmt" | "rate"
            | "macauley_duration" | "convexity" | "yield_to_maturity"
            | "accrued_interest" | "clean_price" | "dirty_price"
            | "coupon_count" | "skill_score" | "reliability_diagram"
            | "taylor_diagram_score"

            // ── GIS — geohash, H3, S2, UTM, projections ─────────
            | "geohash_neighbors" | "h3_index" | "h3_geo_to_h3"
            | "h3_h3_to_geo" | "h3_k_ring" | "h3_neighbor" | "h3_resolution"
            | "s2_cell_id" | "s2_cell_at_lat_lng" | "s2_cell_neighbors"
            | "utm_from_lat_lng" | "utm_to_lat_lng"
            | "mgrs_encode" | "mgrs_decode"
            | "lat_lng_to_xy_mercator" | "lat_lng_to_xy_lambert"
            | "haversine_dist" | "vincenty_dist" | "andoyer_dist"
            | "rhumb_line_bearing"
            | "destination_point" | "tile_xyz_to_lat_lng" | "lat_lng_to_tile_xyz"
            | "polygon_winding_order" | "point_in_polygon_ray"
            | "point_in_polygon_winding" | "segment_intersection"
            | "segment_distance_point" | "convex_hull_chan"

            // ── robotics & control ──────────────────────────────
            | "pid_anti_windup" | "pid_ziegler_nichols"
            | "smith_predictor_step" | "lqr_gain_continuous"
            | "lqr_gain_discrete" | "lqg_step" | "h_infinity_norm"
            | "bode_gain_margin" | "bode_phase_margin"
            | "nyquist_encirclement" | "nichols_chart_step"
            | "servo_position_velocity" | "servo_torque_step"
            | "imu_madgwick_step" | "imu_mahony_step" | "quaternion_from_imu"
            | "denavit_hartenberg_h" | "forward_kinematics_dh"
            | "inverse_kinematics_2link" | "jacobian_2dof"
            | "manipulability_yoshikawa" | "singularity_check_2link"
            | "path_dubins_lsl" | "path_dubins_rsr" | "path_reeds_shepp"
            | "rrt_extend" | "rrt_star_rewire" | "prm_node_connect"

            // ── actuarial science ───────────────────────────────
            | "life_expectancy_e0" | "force_of_mortality" | "select_ultimate"
            | "annuity_due_an" | "annuity_immediate_an"
            | "term_life_a_n_t" | "whole_life_a"
            | "endowment_pure_e" | "endowment_combined_a"
            | "premium_net" | "level_premium"
            | "reserve_prospective" | "reserve_retrospective"
            | "gross_premium_load" | "experience_factor"
            | "mortality_table_q" | "select_period_step"
            | "multi_decrement_q" | "multi_state_pij"
            | "credibility_buhlmann" | "loss_severity_lognormal"
            | "loss_frequency_poisson" | "ruin_probability_lundberg"
            | "cramer_lundberg_step" | "bornhuetter_ferguson"
            | "chain_ladder_step" | "ibnr_estimate" | "run_off_triangle_step"

            // ── epidemiology / public health ────────────────────
            | "r_naught_basic" | "r_effective_t" | "doubling_time_growth"
            | "sirs_step" | "seirs_step" | "susceptible_to_infected"
            | "attack_rate" | "vaccination_coverage_required"
            | "cfr_case_fatality" | "ifr_infection_fatality"
            | "dalys_disability_weight" | "qaly_lifetime" | "ylll_pml"
            | "rt_serial_interval" | "generation_time_step"
            | "gini_inequality_health" | "standardized_mortality_smr"
            | "indirect_age_adjusted" | "direct_age_adjusted"
            | "odds_ratio_2x2" | "risk_ratio_2x2" | "number_needed_to_treat"
            | "attributable_fraction_pop" | "preventive_fraction"
            | "contact_tracing_eff" | "cluster_attack_rate"
            | "transmission_pair_index"

            // ── archive/encoding format primitives ──────────────
            | "tar_header_checksum" | "tar_pad_512" | "tar_member_record"
            | "zip_local_header" | "zip_central_dir" | "zip_eocd"
            | "gzip_member_step" | "gzip_crc32_init" | "gzip_isize"
            | "deflate_dynamic_huffman" | "deflate_static_block"
            | "lz4_block_step" | "lz4_match_offset"
            | "zstd_frame_header" | "brotli_huffman_table"
            | "brotli_meta_block" | "lzma_range_step"
            | "quoted_printable_encode" | "uuencode_step"
            | "modhex_encode" | "percent_encode_full"
            | "punycode_encode" | "idn_to_ascii" | "idn_to_unicode"
            | "msgpack_pack_int" | "msgpack_pack_str"
            | "cbor_encode_uint" | "cbor_encode_str"

            // ── chemistry & biochemistry ────────────────────────
            | "molecular_weight_compound" | "molarity_dilution"
            | "gas_constant_value" | "eyring_rate" | "van_t_hoff_kp"
            | "henderson_buffer" | "titration_ph_endpoint"
            | "isoelectric_point_protein" | "ka_to_pka" | "pkb_to_kb"
            | "amphoteric_check" | "oxidation_number"
            | "half_reaction_balance" | "redox_potential_cell"
            | "electrolysis_mass" | "spectrophotometer_beer_lambert"
            | "epsilon_extinction" | "transmittance_to_a"
            | "crystal_field_ligand" | "jahn_teller_check"
            | "vsepr_geometry" | "lewis_dot_count"
            | "formal_charge" | "resonance_count"
            | "ramachandran_phi_psi" | "rg_radius_of_gyration"
            | "spectroscopic_factor" | "avogadro_constant"

            // ── music theory ────────────────────────────────────
            | "cents_between_freqs" | "note_name_from_midi"
            | "interval_quality_size" | "scale_pitches_major"
            | "scale_pitches_minor" | "mode_pitches_dorian"
            | "mode_pitches_phrygian" | "mode_pitches_lydian"
            | "chord_root_inversion" | "chord_quality_classify"
            | "chord_voicing_close" | "key_signature_sharps"
            | "key_signature_flats" | "tempo_to_ms" | "beat_to_seconds"
            | "time_sig_subdivision" | "equal_tempered_freq"
            | "just_intonation_freq" | "pythagorean_freq"
            | "mean_tone_freq" | "werckmeister_iii" | "kirnberger_iii"
            | "dynamics_db_level" | "harmonics_partial"

            // ── geology, seismology, mineralogy ─────────────────
            | "moment_magnitude_mw" | "richter_local_ml"
            | "surface_wave_ms" | "body_wave_mb"
            | "gutenberg_richter_b" | "omori_aftershock"
            | "pga_attenuation" | "arias_intensity" | "shake_map_pga"
            | "liquefaction_potential_index" | "spt_n_correction"
            | "mineral_mohs_hardness" | "streak_color_index"
            | "specific_gravity_water" | "feldspar_classify"
            | "silicate_classify" | "igneous_qapf"
            | "metamorphic_grade" | "crustal_density_depth"
            | "pwave_velocity_depth" | "swave_velocity_depth"
            | "gradient_geothermal" | "heat_flow_radiogenic"

            // ── BLAS / LAPACK ───────────────────────────────────
            | "dgemm" | "sgemm" | "zgemm" | "cgemm"
            | "dgemv" | "sgemv" | "dtrsm" | "strsm"
            | "dgesv" | "dgetrf" | "dgeqrf" | "dgesvd"
            | "dsyevd" | "dpotrf" | "daxpy" | "ddot"
            | "dnrm2" | "dscal" | "dasum" | "idamax"
            | "dsyrk" | "dgerqf" | "dorgqr" | "dorglq"
            | "drot" | "drotg" | "dpbsv" | "dgbsv"
            | "dtbsv" | "dtrsv" | "ddrot" | "dgemm3m"
            | "dgels" | "dgelsd"

            // ── logic, proof, SAT/SMT, type theory ──────────────
            | "cnf_unit_propagate" | "cnf_pure_literal_elim"
            | "cnf_dpll_branch" | "dpll_clause_learning"
            | "two_watched_literals" | "walksat_step"
            | "resolution_step" | "subsumption_check"
            | "tableau_branch_close" | "sequent_left_intro"
            | "sequent_right_intro" | "nbe_normalize"
            | "church_numeral_n" | "encode_pair" | "encode_succ"
            | "simply_typed_check" | "hindley_milner_step"
            | "unification_robinson" | "bdd_apply" | "bdd_restrict"
            | "bdd_quantify" | "aig_simplify_step"
            | "smt_qf_lia_solve_step" | "smt_qf_uf_combine"
            | "model_checking_ctl" | "model_checking_ltl"
            | "bisimulation_step" | "coq_tactic_apply"
            | "coq_unify_term" | "refl_check" | "sym_check" | "trans_check"

            // ── compilers / parsing ─────────────────────────────
            | "nfa_to_dfa" | "subset_construction"
            | "dfa_minimize_hopcroft" | "regex_to_nfa_thompson"
            | "glushkov_construction" | "brzozowski_derivative"
            | "ll1_first_set" | "ll1_follow_set" | "ll1_predict_table"
            | "lr0_items_step" | "lalr_lookahead_compute"
            | "lr1_canonical_collection"
            | "earley_scan" | "earley_predict" | "earley_complete"
            | "packrat_parse_step" | "ascent_parser_step"
            | "pratt_parse_step" | "shunting_yard_step"
            | "regex_compile_thompson" | "regex_match_dfa"
            | "lex_keyword_classify"
            | "peg_seq" | "peg_choice" | "peg_repeat" | "peg_lookahead"
            | "dfa_simulate_step" | "bytecode_disasm_step"
            | "ssa_phi_insert" | "dom_tree_idom" | "dominance_frontier"

            // ── computational linguistics ───────────────────────
            | "porter_stem_step" | "snowball_stem_english"
            | "snowball_stem_french" | "lemmatize_wordnet"
            | "lemmatize_lemmy" | "stem_lancaster"
            | "soundex_phonetic" | "metaphone_phonetic"
            | "caverphone_2" | "nysiis_phonetic"
            | "match_rating_codex" | "daitch_mokotoff"
            | "viterbi_pos_tag" | "forward_backward_pos"
            | "crf_log_likelihood" | "bigram_perplexity"
            | "trigram_perplexity" | "ner_bilou_decode"
            | "constituency_cyk" | "dependency_parse_eisner"
            | "transition_arc_eager" | "transition_arc_standard"
            | "word_alignment_ibm1" | "word_alignment_ibm2"
            | "lexicalized_parse" | "coreference_singleton"
            | "anaphora_distance" | "head_finding_collins"
            | "tree_kernel_collins"

            // ── Postgres SQL strings, JSON, aggregates ─────────
            | "btrim" | "translate" | "ascii"
            | "regexp_split" | "regexp_matches" | "regexp_replace"
            | "json_build_object" | "jsonb_set"
            | "json_array_length" | "json_extract_path"
            | "json_strip_nulls" | "jsonb_pretty"
            | "jsonb_path_query" | "json_each"
            | "jsonb_array_length" | "jsonb_object_keys"
            | "jsonb_typeof" | "array_to_jsonb"
            | "ts_match" | "ts_rank" | "ts_headline"
            | "substring_similarity" | "levenshtein_dist"
            | "word_similarity" | "strict_word_similarity"
            | "hstore_to_array" | "array_to_hstore"
            | "string_agg" | "array_agg"
            | "corr_agg" | "covar_pop" | "covar_samp"
            | "regr_slope" | "regr_intercept" | "regr_r2"
            | "percentile_cont" | "percentile_disc" | "mode_agg"
            | "array_to_string" | "array_position" | "array_positions"
            | "array_remove" | "array_replace"
            | "xmlforest" | "xmlagg"

            // ── Redis-flavour primitives ────────────────────────
            | "zadd" | "zrem" | "zrangebyscore"
            | "zrank" | "zrevrank" | "zincrby"
            | "zcard" | "zcount" | "zlexcount"
            | "lpush" | "rpush" | "lrange" | "lrem"
            | "hset" | "hget" | "hgetall" | "hlen"
            | "hkeys" | "hvals" | "hmset" | "hincrby"
            | "sadd" | "srem" | "smembers"
            | "sinter" | "sunion" | "sdiff"
            | "scard" | "sismember" | "spop"
            | "setex" | "setnx" | "expire"
            | "ttl" | "pttl" | "persist"
            | "incr" | "decr" | "incrby" | "decrby"
            | "getset" | "mset" | "mget" | "renamenx"
            | "dbsize" | "type_redis" | "exists_key"
            | "strlen" | "getrange" | "setrange" | "append_redis"
            | "bitcount" | "bitop" | "bitpos"
            | "pfadd" | "pfcount"
            | "geoadd" | "geodist" | "geohash"
            | "xadd" | "xlen" | "xrange"
            | "object_encoding" | "debug_object" | "cluster_slots"

            // ── NumPy + scipy.special ──────────────────────────
            | "argpartition" | "bincount" | "nonzero_count"
            | "flatnonzero" | "searchsorted" | "digitize"
            | "histogram_bin_edges" | "unique_count"
            | "polyfit_rmse"
            | "ellipk" | "ellipe"
            | "hyp1f1" | "hyp2f1" | "mathieu_b"
            | "spherical_jn" | "spherical_yn"
            | "jv" | "yn" | "iv" | "kv"
            | "airyai" | "airybi"
            | "polygamma" | "trigamma" | "loggamma"
            | "factorial2" | "factorialk"
            | "owens_t" | "marcum_q" | "voigt_profile"
            | "chebyt" | "chebyu" | "sph_harm"
            | "wofz" | "erfcx" | "erfi" | "dawsn"
            | "interp1d"
            | "convolve_full" | "convolve_valid" | "correlate_full"
            | "kron_product"
            | "simpson_rule" | "romberg_quad" | "fixed_quad"
            | "ode45_step" | "ode_lsoda" | "solve_ivp_step"
            | "root_brentq" | "root_newton" | "root_secant"
            | "fmin_powell" | "fmin_cobyla"

            // ── economics + game theory ─────────────────────────
            | "cobb_douglas" | "ces_production"
            | "leontief_input" | "leontief_output"
            | "slutsky_decompose"
            | "marshallian_demand" | "hicksian_demand"
            | "expenditure_function" | "indirect_utility"
            | "gale_shapley_step" | "deferred_acceptance"
            | "top_trading_cycle" | "vcg_payment" | "myerson_optimal"
            | "gini_market" | "hhi_concentration"
            | "cournot_eq" | "stackelberg_eq" | "bertrand_eq"
            | "monopoly_lerner"
            | "consumer_surplus" | "producer_surplus"
            | "deadweight_loss" | "tax_incidence"
            | "pareto_efficiency" | "edgeworth_box_alloc"
            | "social_welfare_utilitarian"
            | "social_welfare_rawls" | "social_welfare_nash"
            | "arrow_independence"
            | "vickrey_auction" | "first_price_seal"
            | "english_auction" | "dutch_auction"
            | "core_coalition" | "stable_matching_count"
            | "gale_optimal" | "pareto_dominance"
            | "lerner_index"
            | "price_elasticity" | "supply_elasticity"
            | "income_elasticity" | "engel_curve" | "cross_elasticity"
            | "diff_in_diff" | "did_estimator" | "rdd_estimate"
            // ── SciPy.signal — DSP filters, windows, transforms ──
            | "hann_w" | "hamming_w" | "blackman_w" | "barthann_w"
            | "nuttall_w" | "flattop_w" | "parzen_window" | "tukey_w"
            | "taylor_window" | "dpss_window" | "kaiserord_step"
            | "butter_lp_re" | "butter_hp_mag"
            | "cheby1_lp" | "cheby2_lp" | "ellip_lp" | "bessel_lp"
            | "notch_filter"
            | "sosfilt_step" | "lfilter_zi_init" | "filtfilt_pad"
            | "freqz_eval" | "freqs_eval" | "group_delay_eval"
            | "impulse_response_n"
            | "tf2zpk_step" | "zpk2tf_step" | "tf2sos_step"
            | "zpk2sos_step" | "sos2tf_step"
            | "bilinear_xform" | "bilinear_zpk_xform"
            | "firwin_lowpass" | "firwin_highpass"
            | "firwin_bandpass" | "firwin_bandstop"
            | "firwin2_freq" | "remez_design"
            | "stft_step" | "istft_step"
            | "cwt_morlet" | "ricker_wavelet" | "mexican_hat_wavelet"
            | "coherence_xy" | "csd_xy" | "welch_psd_avg"
            | "periodogram_basic" | "lombscargle_freq"
            | "hilbert_signal" | "envelope_amplitude"
            | "deconvolve_step" | "fftconvolve_step" | "oaconvolve_step"
            | "upfirdn_step" | "resample_poly_step" | "decimate_step"
            | "savgol_coef" | "detrend_linear"
            | "wiener_filter" | "medfilt_1d" | "peak_widths_at"
            // ── NetworkX graph algorithms ───────────────────────
            | "dijkstra_relax" | "bellman_ford_relax"
            | "floyd_warshall_step" | "johnson_reweight"
            | "astar_search" | "bidirectional_dijkstra"
            | "yen_k_shortest" | "ida_star"
            | "bfs_count" | "dfs_postorder_done" | "topo_kahn_step"
            | "tarjan_scc_step" | "kosaraju_step"
            | "kruskal_step" | "prim_step" | "boruvka_step"
            | "reverse_delete_step"
            | "ford_fulkerson_step" | "edmonds_karp_bfs"
            | "dinic_step" | "push_relabel_relabel"
            | "stoer_wagner_step" | "karger_step"
            | "pagerank_iter" | "hits_authority" | "hits_hub"
            | "personalized_pagerank"
            | "centrality_degree" | "centrality_closeness"
            | "centrality_betweenness" | "centrality_eigenvector"
            | "centrality_katz" | "harmonic_centrality" | "load_centrality"
            | "clustering_coefficient" | "triangles_count" | "transitivity"
            | "modularity_score" | "louvain_gain"
            | "label_propagation" | "girvan_newman"
            | "articulation_point" | "bridge_edge"
            | "edge_connectivity" | "vertex_connectivity"
            | "biconnected_components"
            | "gx_diameter" | "gx_radius" | "gx_eccentricity"
            | "warshall_step"
            | "tsp_held_karp" | "tsp_nn_step" | "tsp_christofides"
            | "graph_coloring_greedy" | "welsh_powell"
            | "vf2_consistent" | "subgraph_isomorphism"
            | "hungarian_step" | "hopcroft_karp_step"
            | "bron_kerbosch"
            | "min_vertex_cover" | "max_independent_set"
            | "dominating_set_greedy" | "hamiltonian_path"
            | "min_steiner_tree" | "k_shortest_spanning"
            | "random_walk_hitting" | "simrank"
            // ── Pandas DataFrame ops ────────────────────────────
            | "df_groupby" | "df_aggregate" | "df_apply"
            | "df_transform" | "df_pivot" | "df_pivot_table"
            | "df_melt" | "df_stack" | "df_unstack"
            | "df_explode" | "df_get_dummies" | "df_crosstab"
            | "df_merge" | "df_join" | "df_concat"
            | "df_resample" | "df_rolling" | "df_expanding"
            | "df_ewm" | "df_shift" | "df_diff"
            | "df_pct_change" | "df_corr" | "df_cov"
            | "df_corrwith" | "df_describe" | "df_kurtosis"
            | "df_skew" | "df_sem" | "df_mad"
            | "df_dropna" | "df_fillna" | "df_interpolate"
            | "df_replace" | "df_isnull" | "df_notnull"
            | "df_sort_values" | "df_rank" | "df_quantile"
            | "df_value_counts" | "df_sample" | "df_nlargest"
            | "df_nsmallest" | "df_idxmax" | "df_idxmin"
            | "df_clip" | "df_round" | "df_to_datetime"
            | "df_to_timedelta" | "df_to_numeric" | "df_eval"
            | "df_query" | "df_filter" | "df_drop_duplicates"
            | "df_duplicated" | "df_set_index" | "df_reset_index"
            // ── PIL/OpenCV image processing ─────────────────────
            | "image_resize" | "image_grayscale" | "image_threshold"
            | "image_blur_gaussian" | "image_blur_box" | "image_sharpen"
            | "image_edge_canny" | "image_edge_sobel" | "image_edge_laplacian"
            | "image_dilate" | "image_erode" | "image_morphology_open"
            | "image_morphology_close" | "image_histogram" | "image_equalize"
            | "image_clahe" | "image_contrast" | "image_brightness"
            | "image_gamma" | "image_invert" | "image_sepia"
            | "image_posterize" | "image_solarize" | "convolve_2d"
            | "filter_median" | "filter_bilateral" | "filter_nlmeans"
            | "gabor_filter" | "hog_features" | "harris_corners"
            | "shi_tomasi_corners" | "sift_keypoints" | "orb_keypoints"
            | "surf_keypoints" | "template_match" | "face_detect_haar"
            | "watershed_segment" | "slic_superpixels" | "felzenszwalb_segment"
            | "graph_cut_segment" | "hough_lines" | "hough_circles"
            | "ransac_homography" | "optical_flow_lk" | "optical_flow_farneback"
            | "corner_subpix" | "image_rotate" | "image_flip_h"
            | "image_flip_v" | "image_emboss" | "image_motion_blur"
            // ── statsmodels ─
            | "arima_fit" | "arima_forecast" | "arma_order_select"
            | "sarimax_fit" | "garch_fit" | "ewma_smooth"
            | "holt_winters_additive" | "holt_winters_multiplicative" | "kalman_filter_step"
            | "kalman_smoother_step" | "var_fit" | "vecm_fit"
            | "johansen_test" | "phillips_perron" | "adfuller"
            | "kpss_test" | "breusch_godfrey" | "ljung_box_q"
            | "durbin_watson_d" | "granger_causality" | "cointegration_eg"
            | "seasonal_decompose" | "stl_decompose" | "acf_basis"
            | "pacf_basis" | "moving_average_filter" | "exp_smooth_simple"
            | "exp_smooth_double" | "markov_switching_ar" | "markov_switching_mr"
            | "arch_lm" | "state_space_kalman" | "ucm_unobserved_components"
            | "spectral_density_estimate" | "bayesian_step" | "pivoted_cholesky_var"
            // ── sklearn ─
            | "sk_logistic_predict" | "sk_logistic_fit" | "sk_random_forest_fit"
            | "sk_gbt_fit" | "sk_xgb_fit" | "sk_lightgbm_fit"
            | "sk_svm_fit" | "sk_kmeans_fit" | "sk_dbscan_fit"
            | "sk_agglomerative_fit" | "sk_pca_fit" | "sk_tsne_fit"
            | "sk_umap_fit" | "sk_isolation_forest_fit" | "sk_lof_fit"
            | "sk_kfold_split" | "sk_stratified_kfold" | "sk_cross_val_score"
            | "sk_grid_search" | "sk_random_search" | "sk_bayes_search"
            | "sk_pipeline_fit" | "sk_standard_scaler" | "sk_min_max_scaler"
            | "sk_robust_scaler" | "sk_quantile_transform" | "sk_power_transform"
            | "sk_one_hot" | "sk_ordinal_encode" | "sk_label_encode"
            | "sk_tfidf" | "sk_count_vectorize" | "sk_silhouette"
            | "sk_calinski_harabasz" | "sk_davies_bouldin" | "sk_adjusted_rand"
            | "sk_mutual_info" | "sk_lda_topic" | "sk_nmf_topic"
            | "sk_word2vec_train" | "sk_doc2vec_train" | "sk_naive_bayes_predict"
            | "sk_knn_predict" | "sk_decision_tree_split"
            // ── quantum ─
            | "qubit_x" | "qubit_y" | "qubit_z"
            | "qubit_h" | "qubit_s" | "qubit_t"
            | "qubit_rx" | "qubit_ry" | "qubit_rz"
            | "qubit_u3" | "qubit_u2" | "qubit_u1"
            | "qubit_phase" | "qubit_cnot" | "qubit_cz"
            | "qubit_swap" | "qubit_ccx" | "qubit_measure"
            | "qubit_reset" | "bell_state" | "ghz_state"
            | "w_state" | "qft" | "inverse_qft"
            | "grover_iter" | "shor_period" | "vqe_step"
            | "qaoa_step" | "qpe_iteration" | "pauli_string_expect"
            | "circuit_depth" | "circuit_width" | "gate_decompose"
            | "ancilla_alloc" | "bloch_sphere_x" | "bloch_sphere_z"
            | "density_matrix_purity_q" | "entanglement_entropy" | "quantum_teleportation"
            | "superdense_coding" | "noise_model_depolarize"
            // ── b81-misc-utility ─
            | "mirr_excel" | "accrint" | "cumipmt"
            | "cumprinc" | "dollarde" | "dollarfr"
            | "received" | "yieldmat" | "yielddisc"
            | "duration_macaulay" | "mduration" | "odddyield"
            | "disc_excel" | "effect" | "nominal"
            | "intrate" | "price_disc" | "cityhash64"
            | "farmhash_64" | "metro_hash_64" | "spookyhash_128"
            | "t1ha" | "highway_hash" | "fnv0_32"
            | "lose_lose"
            | "oat_hash" | "lz4_encode_block" | "snappy_encode"
            | "zstd_encode_step" | "brotli_encode_meta" | "lzma_encode_step"
            | "bz2_encode_step" | "lzo_encode_step" | "deflate_encode_huffman"
            | "lzw_encode" | "gzip_encode_step" | "uri_template_expand"
            | "uri_resolve" | "uri_normalize" | "percent_decode_url"
            | "url_encode_form" | "url_decode_form" | "punycode_decode_step"
            | "idn_normalize" | "url_origin" | "etag_validate"
            | "cache_control_parse" | "vary_match" | "content_negotiate"
            | "accept_lang_pick" | "range_header_parse" | "if_match_check"
            | "if_none_match_check" | "digest_auth_quote" | "www_auth_parse"
            // ── b82-misc-utility ─
            | "iso8601_duration_parse" | "iso8601_duration_to_seconds" | "rrule_next_occurrence"
            | "cron_next_fire" | "date_round_iso" | "week_number_iso"
            | "fiscal_year_us" | "age_at_date" | "easter_western"
            | "easter_orthodox_year_2" | "chinese_new_year" | "solstice_winter"
            | "equinox_spring" | "rgb_to_oklab" | "oklab_to_rgb"
            | "rgb_to_cmyk" | "cmyk_to_rgb" | "rgb_to_xyz"
            | "xyz_to_rgb" | "rgb_to_yuv" | "yuv_to_rgb"
            | "luminance_relative" | "contrast_ratio" | "wcag_pass"
            | "color_temperature_kelvin" | "delta_e76" | "delta_e94"
            | "delta_e2000" | "color_blend_alpha" | "isbn10_check"
            | "isbn13_check" | "ean13_check" | "upc_check"
            | "eth_addr_check" | "btc_addr_check" | "ssn_check"
            | "vin_check" | "imei_check" | "iban_check"
            | "cusip_check" | "kde_silverman_bw" | "kde_scott_bw"
            | "kde_bandwidth_lscv" | "kde_epanechnikov" | "kde_gaussian_2d"
            | "kde_uniform" | "kde_triangular" | "kde_biweight"
            | "kde_triweight" | "kde_cosine" | "kde_logistic_kernel"
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
            | "pnorm" | "pbinom" | "dbinom" | "ppois"
            | "punif" | "pexp" | "pweibull" | "plnorm" | "pcauchy"
            // ── R base: matrix ops ────────────────────────────────────────
            | "rbind" | "cbind"
            | "row_sums" | "rowSums" | "col_sums" | "colSums"
            | "row_means" | "rowMeans" | "col_means" | "colMeans"
            | "outer" | "crossprod" | "tcrossprod"
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



            // ── R base: quantile functions ────────────────────────────────

            // ── R base: additional CDFs ───────────────────────────────────
            | "pgamma" | "pbeta" | "pchisq" | "pt_cdf" | "pt" | "pf_cdf" | "pf"
            // ── R base: additional PMFs ───────────────────────────────────
            | "dgeom" | "dunif" | "dnbinom" | "dhyper"
            // ── R base: smoothing / interpolation ─────────────────────────
            | "lowess" | "loess" | "approx_fn" | "approx"
            // ── R base: linear models ─────────────────────────────────────
            | "lm_fit" | "lm"
            // ── R base: remaining quantiles ───────────────────────────────
 | "qt_fn" | "qf_fn"

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
            // ── AI primitives (docs/AI_PRIMITIVES.md) ─────────────────
            | "ai" | "ai_agent" | "prompt" | "stream_prompt" | "stream_prompt_cb"
            | "tokens_of"
            | "ai_estimate" | "ai_cost" | "ai_history" | "ai_history_clear"
            | "ai_cache_clear" | "ai_cache_size"
            | "ai_mock_install" | "ai_mock_clear"
            | "ai_config_get" | "ai_config_set" | "ai_routing_get" | "ai_routing_set"
            | "ai_register_tool" | "ai_unregister_tool" | "ai_clear_tools" | "ai_tools_list"
            | "ai_filter" | "ai_map" | "ai_classify" | "ai_match" | "ai_sort" | "ai_dedupe"
            | "ai_extract" | "ai_summarize" | "ai_translate" | "ai_template"
            | "ai_session_new" | "ai_session_send" | "ai_session_history"
            | "ai_session_close" | "ai_session_reset"
            | "ai_session_export" | "ai_session_import"
            | "ai_memory_save" | "ai_memory_recall" | "ai_memory_forget"
            | "ai_memory_count" | "ai_memory_clear"
            | "ai_vision" | "ai_pdf" | "ai_grounded" | "ai_citations"
            | "ai_transcribe" | "ai_speak" | "ai_image" | "ai_image_edit" | "ai_image_variation"
            | "ai_models" | "ai_describe" | "ai_pricing" | "ai_dashboard"
            | "ai_moderate" | "ai_chunk" | "ai_warm" | "ai_compare"
            | "ai_last_thinking" | "ai_budget" | "ai_batch" | "ai_pmap"
            | "ai_file_upload" | "ai_file_list" | "ai_file_get" | "ai_file_delete"
            | "ai_file_anthropic_upload" | "ai_file_anthropic_list" | "ai_file_anthropic_delete"
            | "vec_cosine" | "vec_search" | "vec_topk"
            // ── AI tool specs ────────────────────────────────────────
            | "web_search_tool" | "fetch_url_tool" | "read_file_tool" | "run_code_tool"
            // ── MCP (Model Context Protocol) ─────────────────────────
            | "mcp_connect" | "mcp_close" | "mcp_tools" | "mcp_call"
            | "mcp_resource" | "mcp_resources" | "mcp_prompt" | "mcp_prompts"
            | "mcp_attach_to_ai" | "mcp_detach_from_ai" | "mcp_attached"
            | "mcp_server_start" | "mcp_serve_registered_tools"
            // ── PTY / expect (docs/expect-feature-idea.md) ────────────
            | "pty_spawn" | "pty_send" | "pty_read" | "pty_expect" | "pty_expect_table"
            | "pty_buffer" | "pty_alive" | "pty_eof" | "pty_close" | "pty_interact"
            | "pty_strip_ansi" | "pty_after_eof" | "pty_pending_events"
            // ── Stress / telemetry extensions ─────────────────────────
            | "stress_fp" | "stress_int" | "stress_cache" | "stress_branch"
            | "stress_sort" | "stress_alloc" | "stress_mmap" | "stress_disk"
            | "stress_iops" | "stress_net" | "stress_http" | "stress_dns"
            | "stress_fork" | "stress_thread" | "stress_aes" | "stress_compress"
            | "stress_regex" | "stress_json" | "stress_burst" | "stress_ramp"
            | "stress_oscillate" | "stress_all" | "stress_temp" | "stress_thermal_zones"
            | "stress_freq" | "stress_throttled" | "stress_load" | "stress_meminfo"
            | "stress_cores" | "stress_arm_kill_switch" | "stress_killed"
            | "stress_disarm_kill_switch"
            | "stress_metrics_record" | "stress_metrics_clear" | "stress_metrics_count"
            | "stress_metrics_export" | "stress_metrics_prometheus"
            | "stress_metrics_json" | "stress_metrics_csv" | "stress_metrics_watch"
            // ── Compliance / secrets ─────────────────────────────────
            | "audit_log" | "audit_log_path"
            | "secrets_encrypt" | "secrets_decrypt" | "secrets_random_key" | "secrets_kdf"
            // ── Web framework (docs/WEB_FRAMEWORK.md) ─────────────────
            | "web_route" | "web_resources" | "web_root" | "web_routes_table"
            | "web_application_config" | "web_boot_application"
            | "web_render" | "web_render_partial" | "web_redirect"
            | "web_json" | "web_text" | "web_csv" | "web_markdown"
            | "web_params" | "web_request" | "web_set_header" | "web_status"
            | "web_before_action" | "web_after_action"
            | "web_session" | "web_session_set" | "web_session_get" | "web_session_clear"
            | "web_signed" | "web_unsigned"
            | "web_cookies" | "web_set_cookie"
            | "web_flash" | "web_flash_set" | "web_flash_get"
            | "web_validate" | "web_permit"
            | "web_password_hash" | "web_password_verify"
            | "web_token_for" | "web_token_consume" | "web_csrf_meta_tag"
            | "web_security_headers" | "web_can"
            | "web_h" | "web_truncate" | "web_pluralize" | "web_time_ago_in_words"
            | "web_image_tag" | "web_link_to" | "web_button_to"
            | "web_form_with" | "web_form_close"
            | "web_text_field" | "web_text_area" | "web_check_box"
            | "web_stylesheet_link_tag" | "web_javascript_link_tag"
            | "web_yield_content" | "web_content_for"
            | "web_etag" | "web_cache_get" | "web_cache_set"
            | "web_cache_delete" | "web_cache_clear"
            | "web_db_connect" | "web_db_execute" | "web_db_query"
            | "web_db_begin" | "web_db_commit" | "web_db_rollback"
            | "web_create_table" | "web_drop_table"
            | "web_add_column" | "web_remove_column"
            | "web_migrate" | "web_rollback"
            | "web_model_all" | "web_model_find" | "web_model_first" | "web_model_last"
            | "web_model_where" | "web_model_create" | "web_model_update"
            | "web_model_destroy" | "web_model_count" | "web_model_increment"
            | "web_model_paginate" | "web_model_search" | "web_model_soft_destroy"
            | "web_model_with"
            | "web_jobs_init" | "web_job_enqueue" | "web_job_dequeue"
            | "web_job_complete" | "web_job_fail"
            | "web_jobs_list" | "web_jobs_stats" | "web_job_purge"
            | "web_jsonapi_resource" | "web_jsonapi_collection" | "web_jsonapi_error"
            | "web_bearer_token" | "web_jwt_encode" | "web_jwt_decode"
            | "web_otp_secret" | "web_otp_generate" | "web_otp_verify"
            | "web_uuid" | "web_now" | "web_log" | "web_rate_limit"
            | "web_t" | "web_load_locale" | "web_openapi"
            | "web_faker_int" | "web_faker_email" | "web_faker_name"
            | "web_faker_sentence" | "web_faker_paragraph"
            // ── test runner ─────────────────────────────────────────────────
            // In-process equivalents of `stryke check` / `stryke test`. The
            // builtin form lets stryke programs (e.g. `exercism_run_all.stk`)
            // call them directly without `system "stryke check $f"`, so
            // `check_no_interop` from inside `pmaps` is fork-free and
            // race-free (per-thread no-interop TLS override).
            | "check" | "check_no_interop" | "check_ni"
            | "test" | "test_no_interop" | "test_ni"
            // ── linear algebra / graphs / dates / special math ──
            // ── bits / music theory / hashes / text / statistical tests ──
            // ── phonetic / geo projections / base58/91/z85 / astronomy / crc / color blends / compression ──
            // ── bioinformatics / 3d geometry / sequence alignment / file headers / hmm ──
            // ── game theory / ml inference / chemistry / ops research / info theory ──
            // ── cv kernels / information retrieval / rl / color spaces / windows / trie/fenwick/uf / network ──
            // ── combinatorics / audio synthesis / search / physics 2d / noise / rng variants ──
            // ── ratings / image morphology / computational geometry 2d / crypto / constants / case conversions / photography / unit conversions ──
            | "andrew_monotone_hull" | "aperture_stop_to_fnumber" | "arpad_predict" | "bilateral_filter_2d"
            | "black_hat_transform" | "canny_edges_full" | "case_alternating" | "case_constant"
            | "case_dot" | "case_pascal" | "case_path" | "case_sentence"
            | "case_swap" | "case_title_proper" | "case_train" | "closing_2d"
            | "cohen_sutherland_clip" | "constants_au_meters" | "constants_avogadro_n" | "constants_bohr_radius"
            | "constants_boltzmann_k" | "constants_earth_mass" | "constants_earth_radius" | "constants_electron_charge"
            | "constants_electron_mass" | "constants_faraday" | "constants_gas_r" | "constants_gravitational_g"
            | "constants_lightyear_meters" | "constants_neutron_mass" | "constants_parsec_meters" | "constants_planck_h"
            | "constants_planck_hbar" | "constants_planck" | "constants_proton_mass" | "constants_rydberg"
            | "constants_solar_mass" | "constants_solar_radius" | "constants_speed_of_light" | "constants_stefan_boltzmann"
            | "contour_area" | "contour_centroid" | "contour_find" | "contour_perimeter"
            | "convex_hull_3d" | "convex_hull_3d_simple" | "crop_factor" | "delaunay_triangulate_2d" | "depth_of_field_far"
            | "depth_of_field_near" | "dh_compute_shared" | "dilation_2d" | "ec_point_add"
            | "ec_point_double" | "ed25519_keypair_simple" | "ed25519_sign_simple" | "ed25519_verify_simple"
            | "erosion_2d" | "exposure_value" | "field_of_view" | "focal_length_35mm_equiv"
            | "glicko_rd_update" | "glicko_volatility" | "graham_scan_hull" | "hu_moments"
            | "hyperfocal_distance" | "liang_barsky_clip" | "minkowski_sum_2d" | "moment_image"
            | "morphological_gradient" | "opening_2d" | "pagerank_tournament" | "polygon_inflate"
            | "polygon_offset" | "polygon_self_intersects" | "polygon_shrink" | "polygon_simple_check"
            | "polygon_winding" | "prewitt_x_kernel" | "prewitt_y_kernel" | "ranking_average"
            | "ranking_kendall_tau" | "ranking_spearman_rho" | "roberts_cross_kernel" | "rsa_keypair_simple"
            | "rsa_modular_exp" | "scharr_x_kernel" | "scharr_y_kernel" | "schnorr_sign_simple"
            | "schnorr_verify_simple" | "shutter_speed_reciprocal" | "sobel_magnitude" | "sunny_16_rule"
            | "swiss_pairing" | "top_hat_transform" | "tournament_score" | "trueskill_simple"
            | "unit_energy" | "unit_pressure" | "unit_temperature" | "unit_volume_metric_to_us"
            | "unit_volume_us_to_metric" | "voronoi_cell_2d" | "weiler_atherton_clip" | "zernike_radial"

            | "a_star_grid" | "all_pass_filter" | "am_synth" | "bidirectional_bfs"
            | "buoyancy_force" | "center_of_mass_2d" | "center_of_mass_3d" | "centered_polygonal"
            | "chorus_simple" | "collision_response_2d" | "comb_filter" | "compositions_count"
            | "critical_damping" | "cube_number" | "damping_factor" | "decagonal_number"
            | "derangement_count" | "dodecahedral" | "elastic_collision_1d" | "exponential_search"
            | "fbm_noise_2d" | "fibonacci_matrix" | "fibonacci_nth_fast" | "fir_filter"
            | "flanger_simple" | "floyd_cycle_detect" | "fm_synth_2op" | "freeverb_lite"
            | "gnomonic_number" | "greedy_best_first" | "hash_2d_int" | "heptagonal_number"
            | "hexagonal_number" | "hyperfactorial" | "icosahedral" | "ida_star_search"
            | "inelastic_collision_1d" | "interpolation_search" | "lattice_paths" | "lift_force"
            | "lucas_nth" | "moment_of_inertia_cylinder" | "moment_of_inertia_disc" | "moment_of_inertia_rod"
            | "moment_of_inertia_sphere" | "mulberry32_next" | "multinomial_coefficient" | "narayana_cow"
            | "nonagonal_number" | "octagonal_number" | "partitions_count" | "pcg32_next"
            | "pell_nth" | "perlin_2d" | "perlin_3d" | "phaser_simple"
            | "plate_reverb_simple" | "poisson_brackets" | "primorial" | "projectile_position"
            | "projectile_velocity" | "ridge_noise_2d" | "ring_modulate" | "schroeder_reverb"
            | "simplex_2d" | "splitmix64_next" | "spring_oscillator_pos" | "square_pyramidal"
            | "super_factorial" | "ternary_search" | "tetrahedral" | "tetranacci"
            | "torque_arm" | "turbulence_noise_2d" | "value_noise_2d" | "wavetable_synth"
            | "worley_2d" | "xorshift32_next"

            | "adaptive_threshold" | "bayes_factor" | "bayesian_beta_update" | "bayesian_normal_update"
            | "bm25_score" | "boltzmann_choose" | "braycurtis_dist" | "canberra_dist"
            | "canny_edges_simple" | "chebyshev_norm" | "cidr_to_range" | "ciede2000_color_distance"
            | "ciede76_color_distance" | "ciede94_color_distance" | "conv1d_apply" | "conv2d_apply"
            | "correlate2d" | "cosine_sim_sparse" | "credible_interval_beta" | "credible_interval_normal"
            | "dice_coeff" | "earth_mover_1d" | "epsilon_greedy_choose" | "fenwick_new"
            | "fenwick_query_prefix" | "fenwick_query_range" | "fenwick_update" | "gaussian_kernel"
            | "gradient_magnitude_2d" | "harris_response" | "integral_image" | "ip_subnet_split"
            | "ipv6_global_unicast" | "jaccard_sim" | "laplacian_kernel" | "lch_to_rgb"
            | "mahalanobis_sq" | "manhattan_norm" | "maximum_a_posteriori" | "minkowski_norm"
            | "non_max_suppression" | "oklch_to_rgb" | "otsu_threshold" | "overlap_coeff"
            | "posterior_predictive_beta" | "posterior_predictive_normal" | "prior_jeffreys_uniform" | "qlearning_step"
            | "range_to_cidr" | "rgb_to_lch" | "rgb_to_oklch" | "rl_discount_returns"
            | "rl_n_step_return" | "rl_td_error" | "sarsa_step" | "sliding_dot_product"
            | "sobel_x_kernel" | "sobel_y_kernel" | "softmax_choose" | "tanimoto_coeff"
            | "tfidf_compute" | "thompson_beta_choose" | "trie_count" | "trie_insert"
            | "trie_keys" | "trie_lookup" | "trie_new" | "trie_prefix_search"
            | "trie_remove" | "tversky_index" | "ucb1_choose" | "union_find_components"
            | "union_find_find" | "union_find_new" | "union_find_union" | "window_bartlett"
            | "window_blackman_harris" | "window_flat_top" | "window_gaussian" | "window_welch"

            | "alphabeta_value" | "chem_arrhenius_k" | "chem_avogadro" | "chem_balance_check"
            | "chem_boiling_point_elevation" | "chem_buffer_capacity" | "chem_celsius_to_fahrenheit" | "chem_celsius_to_kelvin"
            | "chem_concentration_to_molarity" | "chem_dilution" | "chem_fahrenheit_to_celsius" | "chem_fahrenheit_to_kelvin"
            | "chem_formula_parse" | "chem_freezing_point_depression" | "chem_h_from_ph" | "chem_henderson_hasselbalch"
            | "chem_ideal_gas_volume" | "chem_isoelectric_estimate" | "chem_kelvin_to_celsius" | "chem_kelvin_to_fahrenheit"
            | "chem_kelvin_to_rankine" | "chem_molality" | "chem_molar_mass" | "chem_molarity_to_normality"
            | "chem_partial_pressure" | "chem_ph_from_h" | "chem_pka_lookup" | "chem_rankine_to_kelvin"
            | "conditional_entropy" | "edmonds_karp_max_flow" | "expectiminimax_value" | "ford_fulkerson_max_flow"
            | "job_schedule_ljf" | "job_schedule_spt" | "joint_entropy" | "js_divergence_distributions"
            | "kl_divergence_distributions" | "knapsack_fractional" | "knapsack_unbounded" | "lp_simplex_max"
            | "lp_simplex_min" | "matching_bipartite_greedy" | "matching_bipartite_hungarian" | "minimax_value"
            | "mixed_strategy_2x2" | "ml_attention_score" | "ml_batch_norm" | "ml_dot_product_attention"
            | "ml_dropout_mask" | "ml_elu_layer" | "ml_gelu_layer" | "ml_hinge_loss"
            | "ml_huber_loss" | "ml_kl_div_loss" | "ml_label_smooth" | "ml_layer_norm"
            | "ml_leaky_relu_layer" | "ml_mae_loss" | "ml_mish_layer" | "ml_mse_loss"
            | "ml_one_hot_encode" | "ml_position_encoding" | "ml_relu_layer" | "ml_self_attention"
            | "ml_sigmoid_layer" | "ml_softmax_layer" | "ml_softplus_layer" | "ml_swish_layer"
            | "ml_tanh_layer" | "ngram_perplexity" | "ngram_prob" | "ngram_top_k_next"
            | "ngram_train" | "payoff_matrix" | "relative_entropy" | "tsp_2opt"
            | "zero_sum_value"

            | "aabb_contains_point" | "aabb_intersects" | "aabb_new" | "aabb_union"
            | "aabb_volume" | "backward_algorithm" | "blast_kmer_index" | "bmp_header_read"
            | "bootstrap_resample" | "codon_optimize" | "codon_to_amino_acid" | "codon_usage_table"
            | "dna_at_content" | "dna_complement" | "dna_gc_content" | "dna_kmer_count"
            | "dna_kmer_index" | "dna_melting_temp" | "dna_reverse_complement" | "dna_transcribe"
            | "dna_translate" | "elf_header_read" | "forward_algorithm" | "gif_header_read"
            | "ico_header_read"
            | "jpeg_markers" | "levenshtein_edit_path" | "mach_o_header_read" | "markov_stationary"
            | "markov_transition_matrix" | "mat4_determinant" | "mat4_identity" | "mat4_inverse"
            | "mat4_look_at" | "mat4_multiply" | "mat4_orthographic" | "mat4_perspective"
            | "mat4_rotate_axis" | "mat4_rotate_x" | "mat4_rotate_y" | "mat4_rotate_z"
            | "mat4_scale" | "mat4_translate" | "mat4_transpose" | "nw_score"
            | "permutation_test" | "plane_distance_to_point" | "plane_normalize" | "png_header_read"
            | "profile_hmm_score" | "protein_charge_at_ph" | "protein_hydrophobicity" | "protein_molecular_weight"
            | "protein_pI" | "quat_conjugate" | "quat_dot" | "quat_from_euler"
            | "quat_identity" | "quat_inverse" | "quat_multiply" | "quat_normalize"
            | "quat_to_euler" | "quat_to_mat4" | "ray_aabb_intersect" | "ray_plane_intersect_2"
            | "ray_plane_intersect" | "rna_gc_content" | "rna_hamming" | "rna_reverse_complement"
            | "rna_to_dna" | "sequence_identity_pct" | "sequence_similarity_pct" | "shuffle_resample"
            | "sphere_aabb_intersect" | "sphere_sphere_intersect" | "sw_score" | "tar_header_read"
            | "triangle_area_3d" | "triangle_normal" | "vec3_add" | "vec3_cross"
            | "vec3_distance" | "vec3_dot" | "vec3_length" | "vec3_lerp"
            | "vec3_normalize" | "vec3_project" | "vec3_reflect" | "vec3_refract"
            | "vec3_scale" | "vec3_sub" | "vec4_add" | "vec4_dot"
            | "vec4_length" | "vec4_scale" | "vec4_sub" | "viterbi_decode"
            | "wav_header_read" | "zip_central_directory" | "zip_local_file_header"

            | "adler32_combine" | "ase_palette_extract" | "base58check_decode" | "base58check_encode"
            | "base91_decode" | "basE91_decode" | "base91_encode" | "basE91_encode"
            | "bwt_invert" | "bwt_transform" | "caverphone" | "caverphone2"
            | "crc10_atm" | "crc12_dect" | "crc24" | "crc32_bzip2"
            | "crc32_jamcrc" | "crc32_mpeg2" | "crc32_xfer" | "crc6_itu"
            | "crc64_ecma" | "crc64_xz" | "delta_decode" | "delta_encode"
            | "destination_lat_lon" | "double_metaphone_primary" | "double_metaphone_secondary" | "fletcher16"
            | "fletcher32" | "fletcher64" | "full_moon_julian" | "fuzzy_substring_match"
            | "gamma_correct" | "gamma_uncorrect" | "geomag_declination" | "huffman_decode"
            | "huffman_encode" | "julian_to_unix" | "lambert_project" | "lat_lon_to_utm"
            | "match_rating_compare" | "mercator_project_x" | "mercator_project_y" | "mercator_unproject_lat"
            | "mercator_unproject_lon" | "modified_julian_date" | "moon_age_days" | "moon_distance_km"
            | "new_moon_julian" | "nysiis" | "phonex" | "rgb_blend_color_burn"
            | "rgb_blend_color_dodge" | "rgb_blend_darken" | "rgb_blend_lighten" | "rgb_blend_multiply"
            | "rgb_blend_normal" | "rgb_blend_overlay" | "rgb_blend_screen" | "rle_compress"
            | "rle_decompress" | "season_of_year" | "sidereal_time_greenwich" | "sidereal_time_local"
            | "solar_noon_unix" | "soundex_v1" | "soundex_v2" | "unix_to_julian"
            | "utm_to_lat_lon" | "utm_zone" | "varint_decode" | "varint_encode"
            | "vincenty_bearing" | "z85_decode" | "z85_encode" | "zigzag_decode"
            | "zigzag_encode"

            | "anova_one_way" | "binomial_test" | "bit_clz"
            | "bit_count_ones" | "bit_count_zeros" | "bit_ctz" | "bit_extract"
            | "bit_first_clear" | "bit_first_set" | "bit_insert" | "bit_last_clear"
            | "bit_last_set" | "bit_log2_int" | "bit_parity" | "bit_reverse_u16"
            | "bit_reverse_u32" | "bit_reverse_u64" | "bit_reverse_u8" | "bit_rotate_left"
            | "bit_rotate_right" | "bit_swap_bytes" | "chi_square_goodness_fit" | "chi_square_independence"
            | "chord_augmented" | "chord_diminished" | "chord_diminished7" | "chord_dominant7"
            | "chord_major" | "chord_major7" | "chord_minor" | "chord_minor7"
            | "crc16_xmodem" | "crc16" | "crc32_zlib" | "crc32c"
            | "crc8" | "detab" | "entab" | "fisher_exact_2x2"
            | "gray_code_decode" | "gray_code_encode" | "hmac_md5_hex" | "hmac_sha1_hex"
            | "hmac_sha256_hex" | "hmac_sha384_hex" | "hmac_sha512_hex" | "indent_block"
            | "interval_name" | "jenkins_hash" | "justify_center" | "justify_left"
            | "justify_right" | "kruskal_wallis" | "ks_test_one_sample" | "ks_test_two_sample"
            | "loose_hash" | "mann_whitney_u" | "midi_to_note_name" | "popcount_u32"
            | "popcount_u64" | "proportion_test" | "rank_data" | "scale_blues"
            | "scale_chromatic" | "scale_dorian" | "scale_harmonic_minor" | "scale_locrian"
            | "scale_lydian" | "scale_major" | "scale_melodic_minor" | "scale_minor"
            | "scale_mixolydian" | "scale_pentatonic" | "scale_phrygian" | "seconds_per_beat"
            | "strip_indent" | "t_test_paired" | "tempo_to_ms_per_beat" | "truncate_middle"
            | "unicode_codepoints" | "wilcoxon_signed_rank" | "word_wrap"

            | "beta_function" | "beta_incomplete" | "date_add_days" | "date_add_months"
            | "date_add_years" | "date_business_days_between" | "date_day" | "date_dayofweek"
            | "date_dayofyear" | "date_days_in_month" | "date_diff_days" | "date_diff_hours"
            | "date_diff_minutes" | "date_diff_seconds" | "date_easter" | "date_first_of_month"
            | "date_hour" | "date_is_leap" | "date_is_weekend" | "date_iso_format"
            | "date_iso_week" | "date_last_of_month" | "date_minute" | "date_month"
            | "date_quarter" | "date_second" | "date_str_to_unix" | "date_unix_to_str"
            | "date_weekofyear" | "date_year" | "ei" | "expint"
            | "gamma_regularized_p" | "gamma_regularized_q" | "graph_articulation_points" | "graph_bellman_ford"
            | "graph_betweenness" | "graph_bfs" | "graph_bridges" | "graph_closeness"
            | "graph_clustering_coefficient" | "graph_color_greedy" | "graph_connected_components" | "graph_cycle_detect"
            | "graph_degree" | "graph_dfs" | "graph_dijkstra" | "graph_eccentricity"
            | "graph_eigenvector_centrality" | "graph_floyd_warshall" | "graph_from_edges" | "graph_has_path"
            | "graph_in_degree" | "graph_is_bipartite" | "graph_is_connected" | "graph_kosaraju"
            | "graph_kruskal_mst" | "graph_out_degree" | "graph_pagerank" | "graph_prim_mst"
            | "graph_shortest_path" | "graph_strongly_connected_components" | "graph_tarjan" | "graph_to_adj_list"
            | "graph_to_adj_matrix" | "graph_topological_sort" | "hypergeom_1f1" | "hypergeom_2f1"
            | "li" | "matrix_adjugate" | "matrix_cholesky_decompose" | "matrix_cofactor"
            | "matrix_cols" | "matrix_concat_h" | "matrix_concat_v" | "matrix_determinant"
            | "matrix_from_cols" | "matrix_get" | "matrix_kronecker" | "matrix_lu_decompose"
            | "matrix_minor" | "matrix_new" | "matrix_norm_frobenius" | "matrix_norm_l1"
            | "matrix_norm_linf" | "matrix_outer_product" | "matrix_qr_decompose" | "matrix_reshape"
            | "matrix_rows" | "matrix_set" | "matrix_submatrix" | "matrix_swap_cols"
            | "matrix_swap_rows" | "matrix_to_string" | "matrix_vec_mul" | "si"
            | "sun_rise_unix" | "sun_set_unix" | "zeta_riemann" | "zodiac_sign"
            // ── quant / technical indicators / time-series / finance / optimization ──
            | "add_seasonality" | "trapezoidal_integrate" | "simpson_integrate"
            | "ode_euler" | "fit_curve_least_squares"
            | "adf_test" | "adx" | "atr" | "bollinger_lower" | "bollinger_middle"
            | "bollinger_upper" | "break_even_price" | "break_even_qty" | "candlestick_pattern_doji"
            | "candlestick_pattern_engulfing" | "candlestick_pattern_evening_star" | "candlestick_pattern_hammer" | "candlestick_pattern_morning_star"
            | "candlestick_pattern_three_black_crows" | "candlestick_pattern_three_white_soldiers" | "cci"
            | "dema" | "diff_pct" | "diff_series"
            | "discount_pct" | "donchian_lower" | "donchian_upper" | "double_exponential_smoothing"
            | "duration_modified" | "ema" | "expanding_mean" | "expanding_sum"
            | "fibonacci_extension" | "fibonacci_retracement" | "finite_difference_central"
            | "finite_difference_forward" | "hma"
            | "hurst_exponent" | "interp_lagrange"
            | "interp_linear" | "kama"
            | "keltner_lower" | "keltner_upper" | "lag_series"
            | "loan_interest_total" | "loan_payment" | "loan_remaining" | "log_returns"
            | "macd_histogram" | "macd_signal" | "macd" | "markup_pct" | "net_present_value" | "obv" | "parabolic_sar" | "pivot_points" | "profit_margin_pct" | "remove_seasonality" | "resistance_level"
            | "roc" | "rolling_kurtosis" | "rolling_max"
            | "rolling_mean" | "rolling_median" | "rolling_min" | "rolling_skew"
            | "rolling_std" | "rolling_sum" | "rolling_var"
            | "rsi" | "shift_series" | "simple_returns" | "sma" | "stoch_rsi" | "support_level"
            | "tema" | "trend_line" | "treynor"
            | "trix" | "true_range" | "twap" | "ulcer_index"
            | "volatility_annualized" | "volatility_realized" | "vwap" | "williams_r"
            | "wma"
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
                | "k"
                | "all"
                | "stryke::builtins"
                | "stryke::perl_compats"
                | "stryke::extensions"
                | "stryke::aliases"
                | "stryke::descriptions"
                | "stryke::categories"
                | "stryke::primaries"
                | "stryke::keywords"
                | "stryke::all"
        )
    }

    /// Check if a UDF name shadows a stryke builtin and error if so.
    /// Called only in non-compat mode — compat mode allows shadowing for Perl 5 parity.
    /// Reserved words that cannot be used as function names because they are
    /// lexer-level operators or language keywords that would be mis-tokenized.
    const RESERVED_FUNCTION_NAMES: &'static [&'static str] = &[
        "y",
        "tr",
        "s",
        "m",
        "q",
        "qq",
        "qw",
        "qx",
        "qr",
        "if",
        "unless",
        "while",
        "until",
        "for",
        "foreach",
        "given",
        "when",
        "else",
        "elsif",
        "do",
        "eval",
        "return",
        "last",
        "next",
        "redo",
        "goto",
        "my",
        "our",
        "local",
        "state",
        "sub",
        "fn",
        "class",
        "struct",
        "enum",
        "trait",
        "use",
        "no",
        "require",
        "package",
        "BEGIN",
        "END",
        "CHECK",
        "INIT",
        "UNITCHECK",
        "and",
        "or",
        "not",
        "x",
        "eq",
        "ne",
        "lt",
        "gt",
        "le",
        "ge",
        "cmp",
    ];

    fn check_udf_shadows_builtin(&self, name: &str, line: usize) -> StrykeResult<()> {
        // Already namespaced (e.g. `Foo::y`) — package context makes the
        // name unambiguous, so it can never shadow a builtin.
        if name.contains("::") {
            return Ok(());
        }
        // Reserved syntactic words (`if`, `while`, `package`, …) break
        // parsing as function names regardless of package.
        if Self::RESERVED_FUNCTION_NAMES.contains(&name) {
            return Err(self.syntax_err(
                format!("`{name}` is a reserved word and cannot be used as a function name"),
                line,
            ));
        }
        // Bare `fn name(...)` inside a non-main `package Foo` registers
        // under `Foo::name`. The user sub is callable only via the
        // fully-qualified `Foo::name(...)` spelling — bare calls always
        // dispatch to the global builtin. Allow the declaration.
        if self.current_package != "main" {
            return Ok(());
        }
        // In `package main` (the default), there's no qualified spelling
        // to "escape" a builtin name. Reject `fn sum {}` here so callers
        // never wonder why bare `sum(1,2,3)` ignored their definition.
        if Self::is_known_bareword(name)
            || Self::is_try_builtin_name(name)
            || crate::list_builtins::is_list_builtin_name(name)
        {
            return Err(self.syntax_err(
                format!(
"`{name}` is a stryke builtin and cannot be redefined in `package main` (declare in a named package and call via `Pkg::{name}(...)`, or pass --compat)"
                ),
                line,
            ));
        }
        Ok(())
    }

    /// Check if a hash name shadows a reserved stryke hash and error if so.
    /// Called only in non-compat mode.
    fn check_hash_shadows_reserved(&self, name: &str, line: usize) -> StrykeResult<()> {
        if Self::is_reserved_hash_name(name) {
            return Err(self.syntax_err(
                format!(
"`%{name}` is a stryke reserved hash and cannot be redefined (this is not Perl 5; pass --compat for Perl 5 mode)"
                ),
                line,
            ));
        }
        Ok(())
    }

    /// Validate assignment to %hash in non-compat mode.
    /// Rejects: scalar, string, arrayref, hashref, coderef, undef, odd-length list.
    fn validate_hash_assignment(&self, value: &Expr, line: usize) -> StrykeResult<()> {
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
            ExprKind::List(items)
                if items.len() % 2 != 0
                    && !items.iter().any(|e| {
                        matches!(
                            e.kind,
                            ExprKind::ArrayVar(_)
                                | ExprKind::HashVar(_)
                                | ExprKind::FuncCall { .. }
                                | ExprKind::Deref { .. }
                                | ExprKind::ScalarVar(_)
                        )
                    }) =>
            {
                return Err(self.syntax_err(
                        format!(
                            "odd-length list ({} elements) in hash assignment — missing value for last key",
                            items.len()
                        ),
                        line,
                    ));
            }
            _ => {}
        }
        Ok(())
    }

    /// Validate assignment to @array in non-compat mode.
    /// Rejects: undef (likely a mistake — use `@a = ()` to empty).
    /// Note: bare scalars like `@a = 2` are allowed since Perl coerces them to single-element lists.
    /// Note: `@a = {hashref}` is allowed as a common pattern for single-element arrays.
    fn validate_array_assignment(&self, value: &Expr, line: usize) -> StrykeResult<()> {
        if let ExprKind::Undef = &value.kind {
            return Err(
                self.syntax_err("cannot assign undef to array — use @a = () to empty", line)
            );
        }
        Ok(())
    }

    /// Validate assignment to $scalar in non-compat mode.
    /// Rejects: list literals (Perl 5 silently returns last element — footgun).
    fn validate_scalar_assignment(&self, value: &Expr, line: usize) -> StrykeResult<()> {
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
    fn validate_assignment(&self, target: &Expr, value: &Expr, line: usize) -> StrykeResult<()> {
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
    fn parse_block_or_bareword_cmp_block(&mut self) -> StrykeResult<Block> {
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
    ) -> StrykeResult<Option<Box<Expr>>> {
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
    fn parse_assign_expr_list_optional_progress(&mut self) -> StrykeResult<(Expr, Option<Expr>)> {
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

    fn parse_one_arg(&mut self) -> StrykeResult<Expr> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let expr = self.parse_expression()?;
            self.expect(&Token::RParen)?;
            Ok(expr)
        } else {
            self.parse_assign_expr_stop_at_pipe()
        }
    }

    /// Bare argument for a Perl-5 named unary operator (`defined`, `length`,
    /// `abs`, `scalar`, `ref`, `keys`, `values`, etc.). Named unary precedence
    /// sits between shift (`<<`/`>>`) and comparison (`<`/`>`), so we parse
    /// only down to shift level. The surrounding `&&` / `||` / `==` / `<` /
    /// equality / logical / ternary stay outside the unary's argument.
    /// Without this `defined $x && Y` mis-parsed as `defined($x && Y)` and
    /// silently returned true whenever `$x` was defined — see the skip-list
    /// debugging write-up. Same scope rule for `length` etc.
    fn parse_named_unary_arg(&mut self) -> StrykeResult<Expr> {
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            let expr = self.parse_expression()?;
            self.expect(&Token::RParen)?;
            Ok(expr)
        } else {
            self.parse_shift()
        }
    }

    fn parse_one_arg_or_default(&mut self) -> StrykeResult<Expr> {
        // Treat a line boundary as a hard arg terminator: if the next
        // token is on a *later* line than the named-unary keyword we
        // just consumed, default the operand to `$_` and stop. Without
        // this, `my $x = uc` followed by `my $y = 5` on the next line
        // mis-parses by silently swallowing `my $y = 5` as the implicit
        // argument to `uc`. Stryke (like Perl/shell) terminates
        // statements at newline; continuation requires explicit `\`.
        // The check skips when the *next* token is itself a binary /
        // postfix operator that legitimately continues the expression
        // (handled by the existing operator stop-list below).
        let prev = self.prev_line();
        if self.peek_line() > prev {
            return Ok(Expr {
                kind: ExprKind::ScalarVar("_".into()),
                line: prev,
            });
        }
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
        // Named-unary precedence: parenless arg only goes down to shift level,
        // so surrounding `eq` / `==` / `?:` / `&&` / `||` stay outside. Without
        // this, `ref $x eq "FOO"` mis-parses as `ref ($x eq "FOO")`.
        // (PARITY-016 — also fixes `length $s == 3 ? "Y" : "N"` etc.)
        self.parse_named_unary_arg()
    }

    /// Array operand for `shift` / `pop`: default `@_`, or `shift(@a)` / `shift()` (empty parens = `@_`).
    fn parse_one_arg_or_argv(&mut self) -> StrykeResult<Expr> {
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

    fn parse_builtin_args(&mut self) -> StrykeResult<Vec<Expr>> {
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
    /// `$h{print}`, `$r->{f}` etc. all yield the string key. Stryke also
    /// auto-quotes the string-comparison and word-logical operator tokens
    /// (`eq`, `ne`, `lt`, `gt`, `le`, `ge`, `cmp`, `and`, `or`, `not`, `x`)
    /// here — the lexer eagerly converts those identifiers to operator tokens,
    /// but inside `{…}` followed by `}` they're plainly hash keys.
    /// Stryke exception: topic-slot barewords (`_`, `_<`, `_0`, `_0<`, …)
    /// resolve to the topic value, not the literal name — `$h{_<}` ≡ `$h{$_<}`.
    fn parse_hash_subscript_key(&mut self) -> StrykeResult<Expr> {
        let line = self.peek_line();
        if let Token::Ident(ref k) = self.peek().clone() {
            if matches!(self.peek_at(1), Token::RBrace) && !Self::is_underscore_topic_slot(k) {
                let s = k.clone();
                self.advance();
                return Ok(Expr {
                    kind: ExprKind::String(s),
                    line,
                });
            }
        }
        if matches!(self.peek_at(1), Token::RBrace) {
            if let Some(s) = Self::operator_keyword_to_ident_str(self.peek()) {
                self.advance();
                return Ok(Expr {
                    kind: ExprKind::String(s.to_string()),
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
    fn parse_pattern_list_until_rparen_or_progress(&mut self) -> StrykeResult<Vec<Expr>> {
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
    fn parse_pattern_list_glob_par_bare(&mut self) -> StrykeResult<Vec<Expr>> {
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
    fn parse_glob_par_or_par_sed_args(&mut self) -> StrykeResult<(Vec<Expr>, Option<Box<Expr>>)> {
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

    pub(crate) fn parse_arg_list(&mut self) -> StrykeResult<Vec<Expr>> {
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

    /// Parse a comma-separated list of slice subscript args. Each arg may be a regular
    /// expression, a closed range (`1:3`, `1..3:2`), or an open-ended Python-style colon
    /// range (`:`, `::`, `:N`, `N:`, `::-1`, `:N:M`, `N::M`, `::M`). Open-ended forms
    /// produce `ExprKind::SliceRange`; closed `1:3` produces `ExprKind::Range` (legacy).
    ///
    /// `is_hash` enables fat-comma-style bareword auto-quoting for endpoints — `{a:c:1}`
    /// treats `a` and `c` as string keys without quoting (cannot be a function call;
    /// use `func():other` if you actually want to invoke).
    pub(crate) fn parse_slice_arg_list(&mut self, is_hash: bool) -> StrykeResult<Vec<Expr>> {
        let mut args = Vec::new();
        let saved_no_pf = self.no_pipe_forward_depth;
        self.no_pipe_forward_depth = 0;
        while !matches!(
            self.peek(),
            Token::RParen | Token::RBracket | Token::RBrace | Token::Eof
        ) {
            let arg = match self.parse_slice_arg(is_hash) {
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

    /// Parse one slice subscript argument (see [`Self::parse_slice_arg_list`]).
    fn parse_slice_arg(&mut self, is_hash: bool) -> StrykeResult<Expr> {
        let line = self.peek_line();

        // Open-start: `:` or `::` immediately
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            return self.finish_slice_range(None, false, is_hash, line);
        }
        if matches!(self.peek(), Token::PackageSep) {
            self.advance();
            return self.finish_slice_range(None, true, is_hash, line);
        }

        // Parse FROM with `:` suppressed inside `parse_range` so it doesn't get
        // consumed as a colon-range there — we want to handle the colon ourselves.
        self.suppress_colon_range = self.suppress_colon_range.saturating_add(1);
        let result = self.parse_slice_endpoint(is_hash);
        self.suppress_colon_range = self.suppress_colon_range.saturating_sub(1);
        let from_expr = result?;

        // Trailing `:` or `::` after the FROM endpoint?
        if matches!(self.peek(), Token::Colon) {
            self.advance();
            return self.finish_slice_range(Some(Box::new(from_expr)), false, is_hash, line);
        }
        if matches!(self.peek(), Token::PackageSep) {
            self.advance();
            return self.finish_slice_range(Some(Box::new(from_expr)), true, is_hash, line);
        }

        Ok(from_expr)
    }

    /// After consuming the first colon (or `::` pair), parse the rest of the slice range.
    /// `double` is true if we just consumed `::` — TO is implicit `None`, the next
    /// expression (if any) is STEP.
    ///
    /// Returns `ExprKind::Range` for fully-closed forms (legacy compatibility) and
    /// `ExprKind::SliceRange` whenever any endpoint is omitted (open-ended).
    fn finish_slice_range(
        &mut self,
        from: Option<Box<Expr>>,
        double: bool,
        is_hash: bool,
        line: usize,
    ) -> StrykeResult<Expr> {
        let (to, step) = if double {
            // `::` so TO is implicit; STEP is whatever (if anything) follows.
            let step_v = self.parse_slice_optional_endpoint(is_hash)?;
            (None, step_v)
        } else {
            // single `:` — parse TO, then optional `:STEP`.
            let to_v = self.parse_slice_optional_endpoint(is_hash)?;
            let step_v = if matches!(self.peek(), Token::Colon) {
                self.advance();
                self.parse_slice_optional_endpoint(is_hash)?
            } else if matches!(self.peek(), Token::PackageSep) {
                return Err(
                    self.syntax_err("Unexpected `::` after slice TO endpoint".to_string(), line)
                );
            } else {
                None
            };
            (to_v, step_v)
        };

        // Closed form (both endpoints present) — produce a regular `Range` so the
        // rest of the compiler/VM keeps reusing existing range-expansion paths.
        if let (Some(f), Some(t)) = (from.as_ref(), to.as_ref()) {
            return Ok(Expr {
                kind: ExprKind::Range {
                    from: f.clone(),
                    to: t.clone(),
                    exclusive: false,
                    step,
                },
                line,
            });
        }

        Ok(Expr {
            kind: ExprKind::SliceRange { from, to, step },
            line,
        })
    }

    /// Parse an optional slice endpoint: returns `None` if the next token closes the slice
    /// arg (`,`, `]`, `}`, or another `:`). Otherwise parses an endpoint expression.
    fn parse_slice_optional_endpoint(&mut self, is_hash: bool) -> StrykeResult<Option<Box<Expr>>> {
        if matches!(
            self.peek(),
            Token::Colon
                | Token::PackageSep
                | Token::Comma
                | Token::RBracket
                | Token::RBrace
                | Token::Eof
        ) {
            return Ok(None);
        }
        self.suppress_colon_range = self.suppress_colon_range.saturating_add(1);
        let r = self.parse_slice_endpoint(is_hash);
        self.suppress_colon_range = self.suppress_colon_range.saturating_sub(1);
        Ok(Some(Box::new(r?)))
    }

    /// Parse a single slice endpoint expression. For hash slices, a bareword `Ident`
    /// followed by `:`, `::`, `,`, `]`, or `}` auto-quotes (fat-comma style); otherwise
    /// fall through to standard expression parsing. For array slices, no auto-quote.
    fn parse_slice_endpoint(&mut self, is_hash: bool) -> StrykeResult<Expr> {
        if is_hash {
            if let Token::Ident(name) = self.peek().clone() {
                if matches!(
                    self.peek_at(1),
                    Token::Colon
                        | Token::PackageSep
                        | Token::Comma
                        | Token::RBracket
                        | Token::RBrace
                ) {
                    let line = self.peek_line();
                    self.advance();
                    return Ok(Expr {
                        kind: ExprKind::String(name),
                        line,
                    });
                }
            }
        }
        self.parse_assign_expr()
    }

    /// Arguments for `->name` / `->SUPER::name` **without** `(...)`. Unlike `die foo + 1`
    /// (unary `+` on `1` passed to `foo`), Perl treats `$o->meth + 5` as infix `+` after a
    /// no-arg method call; we must not consume that `+` as the start of a first argument.
    fn parse_method_arg_list_no_paren(&mut self) -> StrykeResult<Vec<Expr>> {
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

    fn parse_list_until_terminator(&mut self) -> StrykeResult<Vec<Expr>> {
        self.parse_list_until_terminator_inner(false)
    }

    /// Variant of `parse_list_until_terminator` that allows `|>` within arguments.
    /// Used by print-like statements (`p`, `say`, `print`, `printf`) so that
    /// `p @a |> sum` parses as `p(sum(@a))` rather than `sum(p(@a))`, matching
    /// the behavior of `~>` thread-first macro.
    fn parse_list_until_terminator_allow_pipe(&mut self) -> StrykeResult<Vec<Expr>> {
        self.parse_list_until_terminator_inner(true)
    }

    fn parse_list_until_terminator_inner(&mut self, allow_pipe: bool) -> StrykeResult<Vec<Expr>> {
        let mut args = Vec::new();
        // Line of the last consumed token (the keyword / function name that
        // triggered this arg parse).  Used for implicit-semicolon: if no args
        // have been parsed yet and the next token is on a *different* line,
        // treat the newline as a statement boundary and stop.
        let call_line = self.prev_line();
        loop {
            // When `allow_pipe` is false, `|>` terminates the list (preserving
            // left-associativity for chains like `@a |> head 2 |> join "-"`).
            // When true (print-like statements), `|>` is allowed within args.
            let is_terminator = if allow_pipe {
                matches!(
                    self.peek(),
                    Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof
                )
            } else {
                matches!(
                    self.peek(),
                    Token::Semicolon
                        | Token::RBrace
                        | Token::RParen
                        | Token::Eof
                        | Token::PipeForward
                )
            };
            if is_terminator {
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
            // When `allow_pipe` is true, pipe chains are consumed within each
            // argument. When false, `|>` terminates the whole call list, so
            // individual args must not absorb a following `|>`.
            if allow_pipe {
                args.push(self.parse_assign_expr()?);
            } else {
                args.push(self.parse_assign_expr_stop_at_pipe()?);
            }
            if !self.eat(&Token::Comma) {
                break;
            }
        }
        Ok(args)
    }

    /// Body of `+{ ... }` — Perl's force-hashref idiom. The opening `+` and `{`
    /// have already been consumed. Tries the normal `KEY => VAL, …` shape first
    /// (so `+{ a => 1, b => 2 }` is identical to `{ a => 1, b => 2 }`); on
    /// failure falls back to "single list-yielding expression treated as a
    /// flat key/value spread" so `+{ map { (k, v) } LIST }` works without
    /// the user needing a temp `my %h = ...; \%h` shuffle.
    fn parse_forced_hashref_body(&mut self, line: usize) -> StrykeResult<Expr> {
        let saved = self.pos;
        if let Ok(pairs) = self.try_parse_hash_ref() {
            return Ok(Expr {
                kind: ExprKind::HashRef(pairs),
                line,
            });
        }
        // Empty `+{}` is the empty hashref.
        self.pos = saved;
        if matches!(self.peek(), Token::RBrace) {
            self.advance();
            return Ok(Expr {
                kind: ExprKind::HashRef(vec![]),
                line,
            });
        }
        // Single expression — eval as list, flatten into key/value pairs via the
        // existing __HASH_SPREAD__ sentinel that `ExprKind::HashRef` already
        // handles in [`Interpreter::eval_expr`].
        let inner = self.parse_expression()?;
        self.expect(&Token::RBrace)?;
        let sentinel_key = Expr {
            kind: ExprKind::String("__HASH_SPREAD__".into()),
            line,
        };
        Ok(Expr {
            kind: ExprKind::HashRef(vec![(sentinel_key, inner)]),
            line,
        })
    }

    fn try_parse_hash_ref(&mut self) -> StrykeResult<Vec<(Expr, Expr)>> {
        let mut pairs = Vec::new();
        while !matches!(self.peek(), Token::RBrace | Token::Eof) {
            // Perl autoquotes a bareword immediately before `=>` (hash key), even for keywords like
            // `pos`, `bless`, `return` — see Text::Balanced `_failmsg` (`pos => $pos`).
            // Stryke exception: topic-slot barewords (`_`, `_<`, `_0`, `_0<`, `_!N!`, …)
            // resolve to the topic value, not the literal name — `{ _ => 1 }` ≡ `{ $_ => 1 }`.
            let line = self.peek_line();
            let key = if let Token::Ident(ref name) = self.peek().clone() {
                if matches!(self.peek_at(1), Token::FatArrow)
                    && !Self::is_underscore_topic_slot(name)
                {
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
    fn parse_hashref_pairs_until(&mut self, term: &Token) -> StrykeResult<Vec<(Expr, Expr)>> {
        let mut pairs = Vec::new();
        while !matches!(&self.peek(), t if std::mem::discriminant(*t) == std::mem::discriminant(term))
            && !matches!(self.peek(), Token::Eof)
        {
            let line = self.peek_line();
            let key = if let Token::Ident(ref name) = self.peek().clone() {
                if matches!(self.peek_at(1), Token::FatArrow)
                    && !Self::is_underscore_topic_slot(name)
                {
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

    /// Reject `$a` / `$b` references in `--no-interop` mode (lexer catches them
    /// outside double-quoted strings; this catches the in-string interpolation
    /// path which has its own parser bypassing `Token::ScalarVar`).
    fn no_interop_check_scalar_var_name(&self, name: &str, line: usize) -> StrykeResult<()> {
        if crate::no_interop_mode() && (name == "a" || name == "b") {
            return Err(self.syntax_err(
                format!(
                    "stryke uses `_` / `_1` (bareword in code) or `$_` / `$_1` \
                     (sigil inside string interpolation / when whitespace would \
                     change parsing) instead of `${}` (--no-interop is active)",
                    name
                ),
                line,
            ));
        }
        Ok(())
    }

    fn parse_interpolated_string(&self, s: &str, line: usize) -> StrykeResult<Expr> {
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
            if chars[i] == LITERAL_AT_IN_DQUOTE {
                literal.push('@');
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
                    self.no_interop_check_scalar_var_name(&sname, line)?;
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
                        self.no_interop_check_scalar_var_name(&inner, line)?;
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
                        self.no_interop_check_scalar_var_name(&name, line)?;
                        parts.push(StringPart::ScalarVar(name));
                    }
                } else if chars[i].is_alphabetic() || chars[i] == '_' {
                    let mut name = String::new();
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        name.push(chars[i]);
                        i += 1;
                    }
                    // Package-qualified names: `$Foo::x`, `$Foo::Bar::baz`. Mirror
                    // the `$#Foo::a` continuation logic. Without this, `"$Foo::x"`
                    // captures only `Foo` and leaves `::x` as literal text — the
                    // interpolation reads bare `$Foo`, which is undef.
                    while i + 1 < chars.len() && chars[i] == ':' && chars[i + 1] == ':' {
                        name.push_str("::");
                        i += 2;
                        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            name.push(chars[i]);
                            i += 1;
                        }
                    }
                    // `$_<`, `$_<<`, … — outer topic (stryke extension). Also
                    // `$_N<`, `$_N<<` for positional aliases. And the indexed
                    // shortcut `$_<N` ≡ `$_<<<...<` (N chevrons), so `"$_<3"`
                    // and `"$_<<<"` interpolate identically.
                    let is_topic_slot = name == "_"
                        || (name.len() > 1
                            && name.starts_with('_')
                            && name[1..].bytes().all(|b| b.is_ascii_digit()));
                    if is_topic_slot {
                        // Try indexed-ascent first: `<` immediately followed by digits.
                        let try_indexed = chars.get(i) == Some(&'<')
                            && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit());
                        let mut handled_indexed = false;
                        if try_indexed {
                            let mut j = i + 1;
                            while j < chars.len() && chars[j].is_ascii_digit() {
                                j += 1;
                            }
                            let digits: String = chars[i + 1..j].iter().collect();
                            if let Ok(n) = digits.parse::<usize>() {
                                if n >= 1 {
                                    for _ in 0..n {
                                        name.push('<');
                                    }
                                    i = j;
                                    handled_indexed = true;
                                }
                            }
                        }
                        if !handled_indexed {
                            while i < chars.len() && chars[i] == '<' {
                                name.push('<');
                                i += 1;
                            }
                        }
                    }
                    // `--no-interop`: `$a` / `$b` are Perl-isms; reject inside
                    // string interpolation too. Catches both `"$a"` and `"$a[0]"`
                    // / `"$a{k}"` / `"$a->[0]"` because every branch below uses
                    // `name` to build the expression.
                    self.no_interop_check_scalar_var_name(&name, line)?;
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
                    // `&` is the regex-match special var — semantically symmetric with
                    // backtick (`$``) prematch and apostrophe (`$'`) postmatch which
                    // are already handled here. `is_special_scalar_name_for_get` doesn't
                    // currently list `&`/`'`/`` ` `` (those have separate runtime paths
                    // for set/clear under regex updates), so we add them inline.
                    if VMHelper::is_special_scalar_name_for_get(&probe)
                        || matches!(c, '\'' | '`' | '&')
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
                                    self.no_interop_check_scalar_var_name(&name, line)?;
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

    fn expr_to_overload_key(&self, e: &Expr) -> StrykeResult<String> {
        match &e.kind {
            ExprKind::String(s) => Ok(s.clone()),
            _ => Err(self.syntax_err(
                "overload key must be a string literal (e.g. '\"\"' or '+')",
                e.line,
            )),
        }
    }

    fn expr_to_overload_sub(&mut self, e: &Expr) -> StrykeResult<String> {
        match &e.kind {
            ExprKind::String(s) => Ok(s.clone()),
            ExprKind::Integer(n) => Ok(n.to_string()),
            ExprKind::SubroutineRef(s) | ExprKind::SubroutineCodeRef(s) => Ok(s.clone()),
            // Anonymous sub: `use overload "+" => sub { ... };` — promote the
            // anon body into a synthetic top-level SubDecl so the overload
            // table can hold the name like the named-sub case. (PARITY-012)
            ExprKind::CodeRef { params, body } => {
                let id = self.next_overload_anon_id;
                self.next_overload_anon_id = self.next_overload_anon_id.saturating_add(1);
                let name = format!("__overload_anon_{}", id);
                self.pending_synthetic_subs.push(Statement {
                    label: None,
                    kind: StmtKind::SubDecl {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                        prototype: None,
                    },
                    line: e.line,
                });
                Ok(name)
            }
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
pub fn parse_expression_from_str(s: &str, file: &str) -> StrykeResult<Expr> {
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
pub fn parse_block_from_str(s: &str, file: &str, line: usize) -> StrykeResult<Expr> {
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
pub fn parse_slice_indices_from_str(s: &str, file: &str) -> StrykeResult<Vec<Expr>> {
    let mut lexer = Lexer::new_with_file(s, file);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new_with_file(tokens, file);
    parser.parse_arg_list()
}

pub fn parse_format_value_line(line: &str) -> StrykeResult<Vec<Expr>> {
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
        let p = parse_ok("fn foo { 1 }");
        assert_eq!(p.statements.len(), 1);
        match &p.statements[0].kind {
            StmtKind::SubDecl { name, .. } => assert_eq!(name, "foo"),
            _ => panic!("expected SubDecl"),
        }
    }

    #[test]
    fn parse_class_method_expr_body_shorthand() {
        let p = parse_ok("class X { fn adg = \"\" }");
        match &p.statements[0].kind {
            StmtKind::ClassDecl { def } => {
                let m = def.method("adg").expect("adg method");
                let body = m.body.as_ref().expect("body");
                assert_eq!(body.len(), 1);
                match &body[0].kind {
                    StmtKind::Expression(e) => match &e.kind {
                        ExprKind::String(s) => assert!(s.is_empty()),
                        _ => panic!("expected string expr, got {:?}", e.kind),
                    },
                    _ => panic!("expected expression stmt"),
                }
            }
            _ => panic!("expected ClassDecl"),
        }
    }

    #[test]
    fn parse_named_fn_eq_shorthand_with_sig() {
        let p = parse_ok("fn add_one($x) = $x + 1");
        match &p.statements[0].kind {
            StmtKind::SubDecl {
                name, params, body, ..
            } => {
                assert_eq!(name, "add_one");
                assert_eq!(params.len(), 1);
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected SubDecl"),
        }
    }

    #[test]
    fn parse_anon_fn_eq_shorthand_with_sig() {
        let p = parse_ok("my $f = fn($x) = 23");
        match &p.statements[0].kind {
            StmtKind::My(decls) => {
                let init = decls[0].initializer.as_ref().expect("initializer");
                match &init.kind {
                    ExprKind::CodeRef { params, body } => {
                        assert_eq!(params.len(), 1);
                        assert_eq!(body.len(), 1);
                    }
                    _ => panic!("expected CodeRef"),
                }
            }
            _ => panic!("expected My"),
        }
    }

    #[test]
    fn parse_struct_method_eq_shorthand() {
        let p = parse_ok("struct S { fn double($a) = $a * 2 }");
        match &p.statements[0].kind {
            StmtKind::StructDecl { def } => {
                assert_eq!(def.methods.len(), 1);
                assert_eq!(def.methods[0].name, "double");
                assert_eq!(def.methods[0].body.len(), 1);
            }
            _ => panic!("expected StructDecl"),
        }
    }

    #[test]
    fn parse_trait_method_eq_shorthand() {
        let p = parse_ok("trait T { fn k = 0 }");
        match &p.statements[0].kind {
            StmtKind::TraitDecl { def } => {
                let m = def.method("k").expect("k");
                let body = m.body.as_ref().expect("default body");
                assert_eq!(body.len(), 1);
            }
            _ => panic!("expected TraitDecl"),
        }
    }

    #[test]
    fn parse_fn_eq_shorthand_rejects_top_level_comma() {
        let msg = parse_err("fn z = 1, 2");
        assert!(
            msg.contains("single expression") || msg.contains("comma"),
            "{}",
            msg
        );
    }

    #[test]
    fn parse_subroutine_with_prototype() {
        let p = parse_ok("fn foo ($$) { 1 }");
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
        let p = parse_ok("fn Test::counter { state $n = 0; $n++ }");
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
        let p = parse_ok("fn foo { return 42 }");
        assert_eq!(p.statements.len(), 1);
    }

    #[test]
    fn parse_wantarray() {
        let p = parse_ok("fn foo { wantarray ? @a : $a }");
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

    /// `on $cluster () …` must keep `()` as SOURCE (empty list), not postfix
    /// indirect `($cluster)()` which leaves `map` as SOURCE and breaks parsing.
    #[test]
    fn parse_dist_thread_on_scalar_empty_list_source() {
        let p = parse_ok("~d> on $c () map { _ * 2 }");
        assert_eq!(p.statements.len(), 1);
        let StmtKind::Expression(root) = &p.statements[0].kind else {
            panic!("expected Expression statement");
        };
        let ExprKind::DistReduceExpr { cluster, list, .. } = &root.kind else {
            panic!("expected DistReduceExpr, got {:?}", root.kind);
        };
        assert!(
            matches!(cluster.kind, ExprKind::ScalarVar(ref s) if s == "c"),
            "expected cluster $c, got {:?}",
            cluster.kind
        );
        assert!(
            matches!(list.kind, ExprKind::List(ref v) if v.is_empty()),
            "expected empty list source, got {:?}",
            list.kind
        );
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
    fn pipe_sort_does_not_swallow_next_my_decl() {
        // Regression: bare `|> sort` followed by `\n my $x = ...` used
        // to eat the next stmt as sort's argument list. After the fix,
        // both statements must appear in the AST.
        let p = parse_ok("my @s = @data |> sort\nmy $j = join(\",\", @s)");
        assert_eq!(
            p.statements.len(),
            2,
            "expected 2 stmts (sort + join decl), got {}: {:?}",
            p.statements.len(),
            p.statements
                .iter()
                .map(|s| format!("{:?}", s.kind).chars().take(60).collect::<String>())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn pipe_sort_multiline_pipeline_preserves_next_decl() {
        // Same shape but with maps/grep stages between the source and
        // `sort` — mirrors the original `test_oop_inventory_threaded_pin`
        // bug fixture.
        let p = parse_ok(
            "my @bk = @{$inv->by_cat(\"bakery\")} |> maps { _->label() } |> sort\nmy $j = join(\"|\", @bk)",
        );
        assert_eq!(p.statements.len(), 2);
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
        let err = parse_err("fn foo {");
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

    // ── --no-interop strict-mode rejections ─────────────────────────────
    //
    // `--no-interop` is the bot firewall: it rejects Perl 5 idioms the
    // parser would otherwise accept, forcing stryke-only spellings. Each
    // of these pins one rejection rule so a later refactor can't silently
    // accept the un-idiomatic form. We RAII the TLS flag so sibling tests
    // running in parallel don't see the override.

    struct NoInteropGuard {
        saved: Option<bool>,
    }
    impl NoInteropGuard {
        fn on() -> Self {
            let saved = crate::no_interop_mode_tls();
            crate::set_no_interop_mode_tls(Some(true));
            Self { saved }
        }
    }
    impl Drop for NoInteropGuard {
        fn drop(&mut self) {
            crate::set_no_interop_mode_tls(self.saved);
        }
    }

    #[test]
    fn no_interop_rejects_sub_keyword() {
        let _g = NoInteropGuard::on();
        let err = parse_err("sub foo { 1 }");
        assert!(
            err.contains("--no-interop") && err.contains("fn"),
            "sub rejected with fn hint: got {err:?}"
        );
    }

    #[test]
    fn no_interop_rejects_say() {
        let _g = NoInteropGuard::on();
        let err = parse_err("say 1");
        assert!(
            err.contains("--no-interop") && err.contains("`p`"),
            "say rejected with p hint: got {err:?}"
        );
    }

    #[test]
    fn no_interop_rejects_scalar_keyword() {
        let _g = NoInteropGuard::on();
        let err = parse_err("my $n = scalar @x");
        assert!(
            err.contains("--no-interop") && (err.contains("len") || err.contains("cnt")),
            "scalar rejected with len/cnt hint: got {err:?}"
        );
    }

    #[test]
    fn no_interop_rejects_reverse() {
        let _g = NoInteropGuard::on();
        let err = parse_err("my @y = reverse @x");
        assert!(
            err.contains("--no-interop") && err.contains("rev"),
            "reverse rejected with rev hint: got {err:?}"
        );
    }

    /// And the inverse — the stryke spellings (`fn`, `p`, `len`, `rev`)
    /// must parse cleanly under the same flag. A regression that
    /// accidentally rejects the canonical form is just as bad as one
    /// that accepts the Perl 5 form.
    #[test]
    fn no_interop_accepts_stryke_idioms() {
        let _g = NoInteropGuard::on();
        // Each of these used to be the Perl 5 form; stryke's equivalent
        // must parse without error.
        parse_ok("fn foo { 1 }");
        parse_ok("p 1");
        parse_ok("my @x = (1, 2, 3); my $n = len(@x)");
        parse_ok("my @x = (1, 2, 3); my @y = rev(@x)");
    }

    /// `--no-interop` must NOT affect default-mode parsing. Tests run in
    /// parallel; the guard's Drop restores the flag, but verify the
    /// happy-path Perl 5 forms still parse with the flag *off* so we know
    /// the guard mechanics actually restore.
    #[test]
    fn default_mode_still_accepts_perl5_forms() {
        // No guard installed — process default (off in tests).
        parse_ok("sub foo { 1 }");
        parse_ok("say 1");
    }

    /// `$a` / `$b` outside a sort or reduce block is rejected under
    /// `--no-interop` — stryke routes the user to `$_0` / `$_1`
    /// implicit-positional names which work everywhere (including in
    /// sort blocks). Pins the lexer arm at `parser.rs::18964` shape.
    #[test]
    fn no_interop_rejects_bare_dollar_a_dollar_b() {
        let _g = NoInteropGuard::on();
        // Bare reference outside a block context.
        let err = parse_err("my $x = $a + $b");
        assert!(
            err.contains("--no-interop") || err.contains("$_0") || err.contains("$_1"),
            "$a/$b rejected with positional hint: got {err:?}"
        );
    }

    /// And inside a sort block too: stryke's strict mode wants
    /// `$_0` / `$_1` even there (per the temperature_converter and
    /// quicksort_no_interop examples).
    #[test]
    fn no_interop_rejects_dollar_a_inside_sort_block() {
        let _g = NoInteropGuard::on();
        let err = parse_err("my @s = sort { $a <=> $b } (3, 1, 2)");
        assert!(
            err.contains("--no-interop") || err.contains("$_0") || err.contains("$_1"),
            "$a in sort block rejected: got {err:?}"
        );
    }

    /// And the inverse — `$_0` / `$_1` inside a sort block parses
    /// clean under `--no-interop`.
    #[test]
    fn no_interop_accepts_positional_underscore_in_sort_block() {
        let _g = NoInteropGuard::on();
        parse_ok("my @s = sort { $_0 <=> $_1 } (3, 1, 2)");
    }

    // ── stryke-specific grammar pins (parse with the flag on or off) ────

    /// Colon ranges `start:end` are stryke-canonical (not `..`).
    /// `1:10`, `0:N`, `-5:5` all parse.
    #[test]
    fn colon_range_parses_in_for_loop() {
        let _g = NoInteropGuard::on();
        parse_ok("for my $i (1:10) { p $i }");
        parse_ok("my @r = 0:99");
        parse_ok("my @r = -5:5");
    }

    /// Postfix `if` / `unless` / `for` modifiers parse on a statement.
    #[test]
    fn postfix_statement_modifiers_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("p _ for (1, 2, 3)");
        parse_ok("p 1 if 1");
        parse_ok("p 0 unless 0");
    }

    /// Pipe-forward `|>` desugars at parse time — verify it accepts both
    /// the bare-function (`x |> f`) and the block (`x |> { _ * 2 }`) forms.
    #[test]
    fn pipe_forward_accepts_both_function_and_block_rhs() {
        let _g = NoInteropGuard::on();
        parse_ok("my $r = 1:10 |> sum");
        parse_ok("my @r = 1:10 |> maps { _ * 2 }");
        parse_ok("my @r = 1:10 |> grep { _ % 2 == 0 } |> maps { _ + 1 }");
    }

    /// `_0`, `_1`, ... bareword positional params (no sigil).
    #[test]
    fn bareword_positional_underscore_n_parses_in_blocks() {
        let _g = NoInteropGuard::on();
        // `_0` / `_1` inside a sort block: the canonical strict spelling.
        parse_ok("my @s = sort { _0 <=> _1 } (3, 1, 2)");
        // And inside a maps block as the per-item topic.
        parse_ok("my @r = maps { _0 * 2 } (1, 2, 3)");
    }

    /// Declarative types — `struct`, `enum`, `class`, `trait` — must
    /// parse under `--no-interop` (they're stryke extensions, not Perl 5
    /// shapes). Pin each via a minimal declaration.
    #[test]
    fn no_interop_accepts_struct_decl() {
        let _g = NoInteropGuard::on();
        parse_ok("struct Point { x => Int, y => Int }");
    }

    #[test]
    fn no_interop_accepts_enum_decl() {
        let _g = NoInteropGuard::on();
        parse_ok("enum Color { Red, Green, Blue }");
        // Data-carrying variants use the `Variant => Type` shape, not
        // `Variant(Type)` — the latter is reserved for pattern
        // destructuring in match arms.
        parse_ok("enum Maybe { Just => Int, Nothing }");
    }

    #[test]
    fn no_interop_accepts_class_decl_with_methods() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "class Rect {\n    width: Float\n    height: Float\n\n    fn area { $self->width * $self->height }\n}",
        );
    }

    #[test]
    fn no_interop_accepts_trait_decl() {
        let _g = NoInteropGuard::on();
        parse_ok("trait Greeter { fn greet; fn loudly { p \"GREET\" } }");
    }

    /// Compound-assign operators — `||=` defined-or-assign, `//=` exists-
    /// or-assign — are stryke- and Perl-compat and should round-trip.
    /// These are the lazy-init idiom for hash-of-array buckets used in
    /// `csv_summary_no_interop.stk`.
    #[test]
    fn defined_or_assign_compound_operators_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $h = {}; $h->{x} ||= []");
        parse_ok("my $v; $v //= 0");
    }

    /// `+{ ... }` is the unambiguous hashref literal (vs `{ ... }` block).
    /// Used in the CSV demo to push rows.
    #[test]
    fn explicit_hashref_literal_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $row = +{ region => \"north\", qty => 10 }");
        parse_ok("my @rows = (+{ a => 1 }, +{ a => 2 })");
    }

    /// `eval { … }` block + `$@` error-variable inspection is the
    /// canonical exception form used in `rpn_calc_no_interop.stk`.
    #[test]
    fn eval_block_and_dollar_at_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $r = eval { 1 + 2 }; p $@ if $@");
        parse_ok("eval { die \"boom\" }; p $@");
    }

    /// `try { … } catch ($e) { … }` is the stryke-extension exception
    /// shape (Perl-5-on-steroids); must parse under `--no-interop`.
    #[test]
    fn try_catch_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("try { die \"boom\" } catch ($e) { p $e }");
    }

    /// File-test operators (`-d`, `-f`, `-r`, `-e`, …) are unary prefix
    /// ops on a filename or filehandle. Stryke inherits the Perl shape.
    #[test]
    fn file_test_operators_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("p 1 if -d \"/tmp\"");
        parse_ok("p 2 if -f \"/etc/hosts\"");
        parse_ok("p 3 if -e $0");
        parse_ok("my $sz = -s \"/etc/hosts\"");
    }

    /// `~>` (thread-first) and `~>>` (thread-last) macros — stryke's
    /// signature pipeline operators alongside `|>`. Pin the basic
    /// parsing shape.
    #[test]
    fn thread_macros_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $r = ~> 5 +1 *2");
        parse_ok("my @r = ~> (1,2,3) maps { _ * 2 }");
    }

    /// Hash-destructure parameter — `fn f({ a => $a, b => $b })` —
    /// is a stryke-extension signature shape used to unpack a hashref
    /// at call time.
    #[test]
    fn hash_destructure_sub_signature_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn handle({ name => $name, qty => $qty }) { p \"$name x $qty\" }");
    }

    /// Anonymous fn (`fn { ... }`) — the implicit-positional closure
    /// shape. Used as a first-class value: assigned, returned, passed.
    #[test]
    fn anonymous_fn_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $f = fn { _0 * 2 }");
        parse_ok("my @doubled = maps { _0 * 2 } (1, 2, 3)");
    }

    /// Ternary `cond ? a : b` chains for inline branching.
    #[test]
    fn ternary_and_chained_ternary_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $r = $x > 0 ? \"pos\" : \"neg\"");
        parse_ok("my $r = $x > 0 ? \"pos\" : $x < 0 ? \"neg\" : \"zero\"");
    }

    /// Negative indices on array slice — `@arr[-3:-1]` for the last
    /// three elements. Used in the parallel_primes demo.
    #[test]
    fn array_slice_with_negative_indices_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @arr = (1,2,3,4,5); my @tail = @arr[-3:-1]");
        parse_ok("my @arr = (1,2,3); my @last_two = @arr[-2:]");
    }

    /// `state $x` declarations (function-local persistent storage)
    /// used by the memoised-fib demo for the cache hash.
    #[test]
    fn state_variable_declaration_parses() {
        let _g = NoInteropGuard::on();
        // Use clearly-non-builtin fn names (`counter` / `memo` clash
        // with stryke builtins).
        parse_ok("fn my_counter { state $n = 0; $n++; $n }");
        parse_ok("fn my_memo($k) { state %cache; $cache{$k} //= compute($k) }");
    }

    /// `our` declarations are package-globals — still legal in strict
    /// mode (just sub/say/scalar/reverse are rejected, not `our`).
    #[test]
    fn our_declaration_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("our $VERSION = 1.0");
        parse_ok("our @EXPORT = (1, 2, 3)");
    }

    /// Regex binding operators — `=~` (match) and `!~` (negated match).
    /// Used in roman_numerals_no_interop for input validation.
    #[test]
    fn regex_binding_operators_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("p 1 if $s =~ /^\\d+$/");
        parse_ok("p 0 unless $s !~ /[A-Z]/");
        parse_ok("my @m = $s =~ /(\\w+)/g");
    }

    /// Nested data-structure literals — hash of array of hash, the
    /// shape used by csv_summary_no_interop (`%by_region` is a
    /// hash of arrays of hashrefs).
    #[test]
    fn nested_data_structure_literals_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my %h = (a => [1, 2, 3], b => [4, 5])");
        parse_ok("my @rows = (+{ x => 1 }, +{ x => 2 })");
        parse_ok("my %grid = (cells => [+{ row => 1 }, +{ row => 2 }])");
    }

    /// Anonymous fn with explicit params — `fn ($x, $y) { ... }`.
    /// Complements the `fn { _0 + _1 }` implicit-positional form.
    #[test]
    fn anonymous_fn_with_explicit_params_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $add = fn ($x, $y) { $x + $y }");
        parse_ok("my $h = fn ($v, %opts) { p $v; p %opts }");
    }

    /// `package Foo;` and `package Foo::Bar;` declarations — qualified
    /// namespace setup. Still legal in strict mode.
    #[test]
    fn package_declaration_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("package Foo; my $x = 1");
        parse_ok("package Foo::Bar::Baz; our $VERSION = 0.01");
    }

    /// `next` / `last` / `redo` loop-control statements (used in
    /// balanced_brackets / brainfuck / sieve demos to break out of
    /// inner loops).
    #[test]
    fn loop_control_keywords_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("for my $i (1:10) { next if $i % 2; p $i }");
        parse_ok("while (1) { last if $done }");
        parse_ok("my $rerun = 0; for (1:5) { if ($rerun) { redo } }");
    }

    /// Labelled loops + labelled `next` / `last`. Used when you need
    /// to break out of nested loops.
    #[test]
    fn labelled_loops_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("OUTER: for my $i (1:10) { last OUTER if $i > 5 }");
        parse_ok("OUTER: for my $i (1:3) { INNER: for my $j (1:3) { next OUTER if $j > $i } }");
    }

    /// String-repeat operator `x` — `\"-\" x 40` for a separator line,
    /// `(0) x N` for an initialized array. Both shapes appear in the
    /// histogram / sieve / brainfuck demos.
    #[test]
    fn string_repeat_x_operator_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $sep = \"-\" x 40");
        parse_ok("my @zeros = (0) x 100");
        parse_ok("my $bar = \"#\" x $count");
    }

    /// `chomp` / `chop` builtins — mutate-in-place string ops on the
    /// topic or an explicit lvalue. Used in stdin-reading scripts.
    #[test]
    fn chomp_chop_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("chomp(my $line = <STDIN>)");
        parse_ok("my $s = \"hi\\n\"; chomp $s");
        parse_ok("my $t = \"hi\"; chop $t");
    }

    /// Substitution operator `s/pat/repl/flags` — used by
    /// palindrome_no_interop to strip non-alphanumerics.
    #[test]
    fn substitution_operator_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $s = \"abc\"; $s =~ s/b/X/");
        parse_ok("my $s = \"AaBb\"; $s =~ s/[a-z]//g");
        parse_ok("my $s = \"Hello\"; $s =~ s/(.)/\\1\\1/g");
    }

    /// `sprintf` for column-aligned strings — used by every demo's
    /// table output.
    #[test]
    fn sprintf_parses_with_format_specs() {
        let _g = NoInteropGuard::on();
        parse_ok("my $row = sprintf(\"%-10s %5d\", \"foo\", 42)");
        parse_ok("p sprintf(\"%.3f ms\", 1.234)");
        parse_ok("p sprintf(\"%04x\", 255)");
    }

    /// `<STDIN>` diamond reads — list and scalar context. Used by
    /// wordcount / csv_summary / anagram demos.
    #[test]
    fn diamond_stdin_reads_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $line = <STDIN>");
        parse_ok("my @lines = <STDIN>");
        parse_ok("while (my $line = <STDIN>) { p $line }");
    }

    /// Chained method calls — `$obj->foo->bar(arg)`. Used in
    /// build_destroy and class demos.
    #[test]
    fn chained_method_calls_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("$obj->foo->bar");
        parse_ok("my $r = $row->{region}");
        parse_ok("$obj->set(1)->get");
    }

    /// Hash dereference syntaxes — `%$href`, `keys %$href`,
    /// `$href->{key}`, `%{ expr }`. Used by set_ops_no_interop.
    #[test]
    fn hash_deref_forms_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $h = +{a=>1}; my %copy = %$h");
        parse_ok("my $h = +{a=>1}; my @k = keys %$h");
        parse_ok("my $h = +{a=>1}; p $h->{a}");
        parse_ok("my $h = +{a=>1, b=>2}; p len(keys %{$h})");
    }

    /// `unless` postfix on a `die` — the canonical assertion shape
    /// every demo uses for self-tests.
    #[test]
    fn die_unless_assertion_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("die \"x must be 1\" unless 1 == 1");
        parse_ok("die \"empty list\" if len(@x) == 0");
        parse_ok("my $x = 1; die \"hi\" unless $x");
    }

    /// `qw(...)` quote-words literal — the bareword-list shape used in
    /// the older example scripts.
    #[test]
    fn qw_literal_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @w = qw(red green blue)");
        parse_ok("for my $name (qw(Alice Bob Carol)) { p $name }");
    }

    /// Array slice with explicit indices — `@arr[0, 2, 4]`. Different
    /// from range slice `@arr[1:3]`.
    #[test]
    fn array_slice_with_explicit_indices_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @a = (10, 20, 30, 40); my @s = @a[0, 2]");
        parse_ok("my @a = (10, 20, 30); my @s = @a[2, 0, 1]");
    }

    /// Hash slice — `@h{'a', 'b'}` returns the list of values at keys
    /// 'a' and 'b'. Different sigil context than scalar hash access.
    #[test]
    fn hash_slice_with_keys_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my %h = (a=>1, b=>2, c=>3); my @v = @h{'a','c'}");
    }

    /// Bitwise operators — `&`, `|`, `^`, `~`, `<<`, `>>`. Used by
    /// bitops_no_interop. Each binds tighter than the comparison
    /// operators, looser than the arithmetic ones (Perl precedence).
    #[test]
    fn bitwise_operators_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $x = 0xAA; my $y = $x & 0x0F");
        parse_ok("my $x = 1; my $y = $x | 2 | 4");
        parse_ok("my $x = 0xFF; my $y = $x ^ 0x80");
        parse_ok("my $x = 0xAA; my $y = ~$x & 0xff");
        parse_ok("my $x = 1; my $y = $x << 4");
        parse_ok("my $x = 0xF0; my $y = $x >> 2");
    }

    /// `0x`, `0b`, `0o` numeric literal prefixes — hex / binary /
    /// octal. Used freely in bitops + numeric-conversion demos.
    #[test]
    fn numeric_literal_prefixes_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $hex = 0xCAFEBABE");
        parse_ok("my $bin = 0b1101");
        parse_ok("my $hex8 = 0xff & 0x0f");
    }

    /// Negative array index — `$arr[-1]`, `$arr[-2]`. Used by
    /// shunting_yard (`$ops[-1]` to peek stack top).
    #[test]
    fn negative_array_index_parses() {
        let _g = NoInteropGuard::on();
        // `@a` / `@b` array names share the `$a` / `$b` reservation in
        // strict mode — use `@arr` instead.
        parse_ok("my @arr = (1, 2, 3); p $arr[-1]");
        parse_ok("my @stack = (10, 20, 30); p $stack[-1] if len(@stack) > 0");
    }

    /// `last` / `next` / `return` as standalone statements (already
    /// pinned alongside `last LABEL` but not as the lone-line form).
    #[test]
    fn loop_control_standalone_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("while (1) { last }");
        parse_ok("for my $i (1:10) { next if $i == 3 }");
        parse_ok("fn ret { return 42 }");
    }

    /// `shift @args` / `pop @args` — common destructuring shape inside
    /// functions for "first arg" / "last arg" pickoff (used by morse
    /// to peel the mode off `@ARGV`).
    #[test]
    fn shift_pop_on_array_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @a = (1, 2, 3); my $first = shift @a");
        parse_ok("my @a = (1, 2, 3); my $last  = pop @a");
        parse_ok("fn drop_first(@xs) { shift @xs; @xs }");
    }

    /// Special internal names — `__FILE__`, `__LINE__`, `__PACKAGE__`
    /// — accessed by tooling and error reporting.
    #[test]
    fn special_internal_names_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("p __FILE__");
        parse_ok("p __LINE__");
        parse_ok("p __PACKAGE__");
        parse_ok("p __FILE__ . \":\" . __LINE__");
    }

    /// Nested ternary RHS with parens / chained alternatives. Used by
    /// the conway demo's pattern dispatch.
    #[test]
    fn deeply_nested_ternary_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $g = ($p eq \"a\") ? 1 : ($p eq \"b\") ? 2 : ($p eq \"c\") ? 3 : 4");
    }

    /// Array-of-hashref + index-into-hash subscript chain:
    /// `$rows[0]->{name}`, `$rows[-1]->{score}`. Used in csv_summary
    /// and ranking demos.
    #[test]
    fn array_of_hashref_chained_subscript_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @rows = (+{name=>'a',sc=>1}); p $rows[0]->{name}");
        parse_ok("my @rows = (+{n=>10},+{n=>20}); p $rows[-1]->{n}");
    }

    /// Nested `for` loops with two index variables — the 2D grid walk
    /// shape used in conway.
    #[test]
    fn nested_for_loops_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("for my $r (0:5) { for my $c (0:5) { p \"$r,$c\" } }");
    }

    /// Compound assignment `$x .= ...` (string append). Used by every
    /// demo that builds output strings incrementally (brainfuck,
    /// RLE encode, Caesar).
    #[test]
    fn dot_assign_string_append_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $s = \"a\"; $s .= \"b\"");
        parse_ok("my $out = \"\"; $out .= \"x\" for (1:3)");
    }

    /// `unshift @arr, $v` — push to the FRONT of an array; used by
    /// graph_bfs for back-tracking the path.
    #[test]
    fn unshift_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @path = (3, 4); unshift @path, 2; unshift @path, 1");
        parse_ok("my @q; unshift @q, $_ for (1:5)");
    }

    /// `defined` and `// 0` defined-or fallback. Used by the
    /// graph_bfs neighbour lookup (`$REVERSE{$ch} // 0`).
    #[test]
    fn defined_or_fallback_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $x; my $y = $x // 0");
        parse_ok("my %h = (a=>1); my $v = $h{missing} // -1");
        parse_ok("p 1 if defined $foo");
    }

    /// `for my $x (rev 0:N)` — reverse a range with the stryke `rev`
    /// keyword. Used by knapsack back-tracking.
    #[test]
    fn rev_over_range_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("for my $i (rev 0:9) { p $i }");
        parse_ok("my @r = rev (1, 2, 3, 4)");
    }

    /// Array deref `@$ref`, `@{$expr}`. Used freely in graph_bfs +
    /// knapsack to walk arrays-of-arrayrefs.
    #[test]
    fn array_deref_forms_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $r = [1, 2, 3]; my @copy = @$r");
        parse_ok("my $r = [1, 2, 3]; my @copy = @{$r}");
        parse_ok("my @rs = ([1], [2, 3]); my @flat; push @flat, @$_ for @rs");
    }

    /// Hashref via `[k]` doesn't exist — but `$ref->{k}` and `${$ref}{k}`
    /// are the two equivalent shapes. Pin both.
    #[test]
    fn hashref_subscript_alt_forms_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $h = +{a=>1}; p $h->{a}");
        parse_ok("my $h = +{a=>1}; p ${$h}{a}");
    }

    /// `abs($x)`, `int($x)`, `sqrt($x)` — common numeric builtins
    /// invoked as functions.
    #[test]
    fn numeric_builtins_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $x = abs(-5)");
        parse_ok("my $f = int(3.7)");
        parse_ok("my $s = sqrt(2)");
        parse_ok("my $n = int($x * 100 + 0.5) / 100");
    }

    /// `local $var` declaration — dynamic-scoped binding restored
    /// on block exit. Different from `my` (lexical) and `our`
    /// (package-global).
    #[test]
    fn local_declaration_parses() {
        let _g = NoInteropGuard::on();
        // Strict mode rejects `sub` — use anonymous `fn` for the scope.
        parse_ok("our $g = 1; (fn { local $g = 99; p $g })->()");
    }

    /// `srand($seed)` and `rand($n)` — deterministic random with
    /// optional bound. Used by dice + histogram demos for repeatable
    /// CI output.
    #[test]
    fn srand_rand_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("srand(42); my $r = rand(6)");
        parse_ok("srand(); my $r = int(rand(100))");
    }

    /// `substr($s, $i, $n)` for slicing strings (2-arg + 3-arg forms).
    /// Used by base64 + soundex + RPN demos.
    #[test]
    fn substr_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $s = \"hello\"; p substr($s, 0, 1)");
        parse_ok("my $s = \"hello\"; p substr($s, 1)");
        parse_ok("my $s = \"hello\"; p substr($s, -2)");
    }

    /// Underscore separator in numeric literals — `1_000_000` for
    /// readability. Used in `pmap { ... } 1:1_000` style code.
    #[test]
    fn underscore_separators_in_numbers_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("my $n = 1_000_000");
        parse_ok("my $r = 1_000_000 / 365");
        parse_ok("my $hex = 0xff_ff");
    }

    /// 2D array-of-arrayref construction `[[a,b], [c,d]]` and access
    /// `$grid->[0]->[1]`. Used by interval_merge and dijkstra demos.
    #[test]
    fn arrayref_of_arrayref_access_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $grid = [[1, 2], [3, 4]]");
        parse_ok("my $grid = [[1, 2], [3, 4]]; p $grid->[0]->[1]");
        parse_ok("my $grid = [[1, 2], [3, 4]]; p $grid->[1][0]");
    }

    /// Mutating array element via arrow-arrow chain — `$ref->[0]->[1] = $v`.
    /// Used by interval_merge to extend an interval's end in place.
    #[test]
    fn arrow_chain_assignment_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $g = [[0, 0]]; $g->[0]->[1] = 99");
        parse_ok("my $g = [[0, 0]]; $g->[0][1] = 99");
    }

    /// Tuple destructure from arrayref element — `my ($a, $b) = @$e`.
    /// Pin the no-interop renames (`$a` reserved).
    #[test]
    fn tuple_destructure_from_arrayref_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $e = [10, 20]; my ($lhs, $rhs) = @$e");
        parse_ok("my $e = [\"x\", 1]; my ($name, $weight) = ($e->[0], $e->[1])");
    }

    /// `next unless` / `last unless` postfix on a loop body. Common
    /// guard shape inside the rolling-stats sliding loops.
    #[test]
    fn next_last_unless_postfix_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("for my $i (1:10) { next unless $i % 2 == 0; p $i }");
        parse_ok("while (1) { last unless $live }");
    }

    /// `keys %{ ... }` with parenthesised hash dereference — the
    /// shape used by deepish hash-of-hash access.
    #[test]
    fn keys_on_braced_hash_deref_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $h = +{a=>1}; my @k = keys %{$h}");
        parse_ok("my $h = +{a=>+{b=>1}}; my @k = keys %{$h->{a}}");
    }

    /// Namespaced fn definition — `fn Module::method($x) { ... }`.
    /// All UDFs in the examples/ demos use this form so future stryke
    /// stdlib additions don't shadow user code (or vice versa).
    #[test]
    fn namespaced_fn_decl_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn Module::method($x) { $x * 2 }");
        parse_ok("fn Foo::Bar::helper { 42 }");
        parse_ok("fn Demo::run { p \"running\" }");
    }

    /// Calling a namespaced fn — `Module::method($arg)`. Also used as
    /// a method on an explicit invocant in some contexts.
    #[test]
    fn namespaced_fn_call_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn Module::add($x, $y) { $x + $y } p Module::add(2, 3)");
        // `caller` itself is a stryke builtin — use a namespaced caller
        // to avoid the clash.
        parse_ok("fn Foo::Bar::baz { 1 } fn Demo::main { Foo::Bar::baz() + Foo::Bar::baz() }");
    }

    /// `index($haystack, $needle)` builtin — the canonical string
    /// substring-position lookup that KMP cross-checks against.
    #[test]
    fn index_builtin_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $i = index(\"hello world\", \"world\")");
        parse_ok("my $i = index($s, $pat, 0)");
    }

    /// `for my $i (rev 1:N)` — reverse iteration over a colon range.
    /// Already pinned in `rev_over_range_parses` but here for the
    /// `rev (list)` form on an array literal.
    #[test]
    fn rev_on_array_literal_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @r = rev (1, 2, 3, 4)");
        parse_ok("for my $x (rev (\"a\", \"b\", \"c\")) { p $x }");
    }

    /// Numeric comparison + string comparison side by side — the
    /// pattern used by sort blocks with secondary tie-breaking
    /// (numeric primary, string secondary). `$a` / `$b` are reserved
    /// under --no-interop; use `$_0` / `$_1` for sort comparator args.
    #[test]
    fn numeric_and_string_comparison_in_one_expr_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $r = $_0 <=> $_1 || $name cmp $other");
        parse_ok("p 1 if $x == $y && $name eq \"foo\"");
    }

    /// Bareword positional names — `_0`, `_1`, `_N` without sigil —
    /// are stryke's idiomatic spelling in code contexts. The sigil
    /// form (`$_0`, `$_1`) is reserved for string interpolation where
    /// a bareword would just be a literal substring. Pin the bareword
    /// shape in every common closure position.
    #[test]
    fn bareword_positional_in_sort_reduce_blocks_parses() {
        let _g = NoInteropGuard::on();
        // Sort comparator with bareword positional names.
        parse_ok("my @s = sort { _0 <=> _1 } (3, 1, 2)");
        // Reduce accumulator + element.
        parse_ok("my $r = (1, 2, 3) |> reduce { _0 + _1 }");
        // Reduce on string concat — _0 acc, _1 next.
        parse_ok("my $s = (\"a\", \"b\") |> reduce { _0 . _1 }");
    }

    /// Bareword topic `_` inside `maps` / `grep` / `pmap` / `pgrep`
    /// closure bodies. Tightest form; no sigil needed.
    #[test]
    fn bareword_topic_in_maps_grep_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @r = (1, 2, 3) |> maps { _ * 2 }");
        parse_ok("my @r = (1, 2, 3, 4) |> grep { _ % 2 == 0 }");
        parse_ok("my @r = 1:100 |> pmap { _ ** 3 }");
        parse_ok("my @r = 1:100 |> pgrep { _ % 7 == 0 }");
    }

    /// `_` and `_1` in arithmetic expression context (no sigil).
    /// Inside string interpolation, only the sigil form `${_}` /
    /// `$_0` / `$_1` works — pin the contrast.
    #[test]
    fn bareword_vs_sigil_in_string_interp_parses() {
        let _g = NoInteropGuard::on();
        // Bareword in code: tight, idiomatic.
        parse_ok("my @r = (5, 10) |> maps { _ + 1 }");
        // Sigil in string interp: the bareword would just be the
        // literal characters underscore-zero, so the sigil form is
        // required here.
        parse_ok("p \"first=$_0 second=$_1\"");
        parse_ok("p \"got: $_\"");
    }

    /// Bareword `_` topic inside a `for $_` -style postfix-for body.
    /// Used in Conway's life-counter — `$n += _ for @$g`.
    #[test]
    fn bareword_topic_in_postfix_for_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $n = 0; $n += _ for (1, 2, 3, 4)");
        parse_ok("my %h; $h{_}++ for (\"a\", \"b\", \"a\")");
    }

    /// Bareword `_` with hash-subscript on the outer side —
    /// `$h{_}++` is tighter than `$h{$_}++` (no sigil on key).
    #[test]
    fn bareword_topic_as_hash_key_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my %h; $h{_}++ for (\"a\", \"b\", \"a\")");
        parse_ok("my @arr = (1, 2, 3); my %seen; $seen{$arr[_]}++ for (0, 1, 2)");
    }

    /// Hashref subscript inside a `grep` block using the bareword
    /// topic — the FirstU::pipe_find shape: `grep { $seen{$c[_]} == 1 }`.
    #[test]
    fn bareword_topic_inside_subscript_chain_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @c = (\"a\", \"b\", \"c\"); my %seen = (a => 1, b => 2, c => 1); \
             my @hits = grep { $seen{$c[_]} == 1 } (0, 1, 2)",
        );
    }

    /// `ref($x)` builtin — used by flatten to discriminate arrayref
    /// from scalar leaves.
    #[test]
    fn ref_builtin_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $x = [1, 2]; p ref($x)");
        parse_ok("my $r = +{a => 1}; p 1 if ref($r) eq \"HASH\"");
    }

    /// Expression-bodied recursive `fn` — style guide rule 6. The body
    /// is a single ternary that re-invokes the same fn; Kadane / gcd /
    /// lcm demos all use this shape.
    #[test]
    fn expression_bodied_recursive_fn_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn N::gcd = _1 == 0 ? _0 : N::gcd(_1, _0 % _1)");
        parse_ok("fn N::lcm = _0 * _1 / N::gcd(_0, _1)");
    }

    /// Expression-bodied `fn` whose body is a `|> reduce` over the
    /// implicit topic — used by gcd_list / lcm_list.
    #[test]
    fn expression_bodied_pipe_reduce_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn N::gcd = _1 == 0 ? _0 : N::gcd(_1, _0 % _1); \
                  fn N::gcd_list = @{_} |> reduce { N::gcd(_0, _1) }",
        );
    }

    /// Reduce-fold with a hashref accumulator seeded by prepending the
    /// init to the input — Boyer-Moore / Kadane idiom.
    #[test]
    fn reduce_fold_with_hashref_accumulator_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @xs = (3, 2, 3); \
             my $st = (+{cur => 0, best => -100}, @xs) |> reduce { \
                +{ cur => _1, best => _0->{best} } \
             }",
        );
    }

    /// Native array-deref slicing `@$arr[$lo:$hi]` — used by
    /// rolling_stats to slice the input window.
    #[test]
    fn array_deref_slice_with_variable_bounds_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @a = (10, 20, 30, 40, 50); my $r = \\@a; \
             my $lo = 1; my $hi = 3; \
             my @s = @$r[$lo:$hi]",
        );
    }

    /// `min(@$v[$lo:$hi])` / `max(@$v[$lo:$hi])` — rolling_stats hot
    /// path. Paren-less `min @$v[..]` parses wrong; pinned with parens.
    #[test]
    fn min_max_over_array_deref_slice_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @a = (1, 3, 2, 5); my $r = \\@a; \
             my $lo = 0; my $hi = 2; \
             my $m1 = min(@$r[$lo:$hi]); \
             my $m2 = max(@$r[$lo:$hi])",
        );
    }

    /// `flat_maps` with recursive call inside the block — flatten
    /// demo's central idiom; verifies the recursive call inside a
    /// pipeline stage parses cleanly.
    #[test]
    fn flat_maps_with_recursive_call_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn Flat::flatten($r) = @$r |> flat_maps { \
                ref(_) eq \"ARRAY\" ? Flat::flatten(_) : (_) \
             }",
        );
    }

    /// Inner `map { _->[$i] }` capturing outer lexical `$i` — zip's
    /// N-list helper. Bareword topic inside, `$i` is the lexical.
    #[test]
    fn nested_map_with_outer_lexical_capture_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @lists = ([1,2,3], [10,20,30]); \
             my @r = 0:2 |> maps { my $i = _; [map { _->[$i] } @lists] }",
        );
    }

    /// `@{_}` deref of the topic — required when sigil-form `@$_` is
    /// being avoided per style guide. zip's `len(@{_})` shape.
    #[test]
    fn array_deref_of_bareword_topic_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @lists = ([1,2,3], [10,20,30]); p min(map { len(@{_}) } @lists)");
    }

    /// `~>` thread-macro stages for `glob`, `rand`, `srand` — these
    /// builtins have their own `ExprKind` (Glob, Rand, Srand), not the
    /// generic FuncCall path, so they previously fell through to the
    /// default arm and produced "Undefined subroutine" at runtime.
    #[test]
    fn thread_macro_accepts_glob_rand_srand_stages() {
        parse_ok("my @r = ~> \"/tmp/*\" glob sort");
        parse_ok("my $i = ~> 100 rand int");
        parse_ok("~> 42 srand");
    }

    /// Recursive expression-bodied `fn` with path compression — the
    /// union-find demo idiom: `fn UF::find($uf, $x) { ... }` body
    /// re-invokes itself and writes the result into a hashref slot.
    #[test]
    fn recursive_fn_with_arrayref_assignment_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn UF::find($uf, $x) { \
                return $x if $uf->{parent}[$x] == $x; \
                $uf->{parent}[$x] = UF::find($uf, $uf->{parent}[$x]); \
                $uf->{parent}[$x] \
             }",
        );
    }

    /// Hash-initialised with computed list inside arrayref literal —
    /// `[0:$n - 1]` and `[(0) x $n]` as field values. Used by UF::new.
    #[test]
    fn hashref_init_with_range_and_repeat_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn UF::new($n) = +{ parent => [0:$n - 1], rank => [(0) x $n], count => $n }");
    }

    /// Postfix-for over an arrayref-deref: `Trie::insert($t, $_) for @$words`.
    /// Used by Trie::from_words.
    #[test]
    fn postfix_for_arrayref_deref_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @words = (\"a\", \"b\"); my $r = \\@words; my @out; push @out, $_ for @$r");
    }

    /// Tuple-swap destructure inside a block — UF::union flips ra/rb
    /// for union-by-rank.
    #[test]
    fn tuple_swap_destructure_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $ra = 1; my $rb = 2; ($ra, $rb) = ($rb, $ra)");
    }

    /// 2-D array initialised with explicit row arrayrefs — the
    /// `--no-interop` mode rejects 2-D autoviv (`$d[$i][$j] = X`
    /// on un-initialized `@d`), so each row must be an arrayref
    /// literal first. Damerau-Levenshtein demo's matrix setup.
    #[test]
    fn explicit_2d_array_row_init_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @d; my $m = 3; my $n = 4; \
             for my $i (0:$m) { $d[$i] = [(0) x ($n + 1)] }",
        );
    }

    /// `min()` with 3 arguments — Damerau-Levenshtein uses this for the
    /// deletion/insertion/substitution step.
    #[test]
    fn min_with_three_args_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $x = min(1, 2, 3)");
        parse_ok("my @d; $d[0][0] = 5; my $r = min($d[0][0] + 1, $d[0][0] + 1, $d[0][0] + 0)");
    }

    /// String slice with `$s[N:M]` where M is `len(...)`-based.
    /// Trie::count_with_prefix shape.
    #[test]
    fn string_slice_with_len_bound_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my $w = \"apple\"; my $pre = \"app\"; \
             my $ok = len($w) >= len($pre) && $w[0:len($pre) - 1] eq $pre",
        );
    }

    /// Sort comparator with `_0->[N] <=> _1->[N]` — sorting an
    /// array-of-arrayrefs by a positional field. Kruskal MST pattern.
    #[test]
    fn sort_block_with_arrow_deref_topic_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @edges = ([0,1,4], [2,3,1], [1,2,2]); \
             my @sorted = sort { _0->[2] <=> _1->[2] } @edges",
        );
    }

    /// C-style `for` header with declarations and post-decrement —
    /// Knuth shuffle's inner loop walks high-to-low.
    #[test]
    fn cstyle_for_with_postdecrement_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @arr = (1, 2, 3, 4, 5); \
             my $n = len(@arr); \
             for (my $i = $n - 1; $i > 0; $i--) { p $arr[$i] }",
        );
    }

    /// Tuple-swap on array-deref index pairs — Knuth-shuffle inner
    /// step swaps `$r->[$i]` and `$r->[$j]` via destructure.
    #[test]
    fn tuple_swap_arrayref_index_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @arr = (1, 2, 3); my $r = \\@arr; my $i = 0; my $j = 2; \
             ($r->[$i], $r->[$j]) = ($r->[$j], $r->[$i])",
        );
    }

    /// Recursive backtracking — N-queens pushes a column, recurses,
    /// then pops. Verifies array mutation inside recursive fn calls.
    #[test]
    fn recursive_backtracking_arrayref_mutation_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn Q::go($n, $cols, $count_ref) { \
                my $r = len(@$cols); \
                if ($r == $n) { $$count_ref++; return } \
                for my $c (0:$n - 1) { \
                    push @$cols, $c; \
                    Q::go($n, $cols, $count_ref); \
                    pop @$cols \
                } \
             }",
        );
    }

    /// Doubly-linked list as a hash-of-{prev,next} — LRU cache's
    /// node-table pattern. `$nodes->{$k}{prev}` chain.
    #[test]
    fn nested_hashref_chain_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my $c = +{ nodes => +{ a => +{ val => 1, prev => undef, next => \"b\" } } }; \
             my $p = $c->{nodes}{a}{prev}; \
             my $n = $c->{nodes}{a}{next}",
        );
    }

    /// `$$count_ref++` — dereference a scalar-ref and post-increment.
    /// Used in Queens::recur to update a shared counter.
    #[test]
    fn scalar_ref_postincrement_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $n = 0; my $ref = \\$n; $$ref++; p $n");
    }

    /// `$h{$k}` autoviv chain — Markov bigram table builds a
    /// hash-of-hash where the inner is created on first access.
    #[test]
    fn hash_of_hash_autoviv_increment_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my %table; \
             my $prev = \"the\"; my $next = \"quick\"; \
             $table{$prev} //= +{}; \
             $table{$prev}{$next}++",
        );
    }

    /// `for (... ; ...) { ... }` C-style with literal counter — Knuth
    /// shuffle's high-to-low walk and similar.
    #[test]
    fn cstyle_for_with_literal_bounds_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $sum = 0; for (my $i = 0; $i < 10; $i++) { $sum += $i }");
    }

    /// Recursive expression-bodied `fn` with ternary base case —
    /// Josephus closed-form. Single-letter tail segments in namespaced
    /// names (`J::s`, `Foo::m`, `Foo::q`, `Foo::qx`, `Foo::qr`) are
    /// identifiers — the `::` prefix disambiguates from the
    /// `s/.../.../`, `m//`, `q//`, etc. quote-like operators.
    #[test]
    fn recursive_expression_body_with_ternary_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn J::s($n, $k) = $n == 1 ? 0 : (J::s($n - 1, $k) + $k) % $n");
    }

    /// All quote-like single/two-letter operators (`s`, `m`, `q`, `qq`,
    /// `qx`, `qr`, `tr`, `y`) are valid namespaced identifier tails
    /// after `::` — they're not lexed as their regex/quote forms.
    #[test]
    fn namespaced_quote_like_tail_segments_parse() {
        let _g = NoInteropGuard::on();
        parse_ok("fn Foo::s($x) = $x + 1");
        parse_ok("fn Foo::m($x) = $x * 2");
        parse_ok("fn Foo::q($x) = $x");
        parse_ok("fn Foo::qq($x) = $x");
        parse_ok("fn Foo::qx($x) = $x");
        parse_ok("fn Foo::qr($x) = $x");
        parse_ok("fn Foo::tr($x) = $x");
        parse_ok("fn Foo::y($x) = $x");
    }

    /// `splice @arr, $i, 1` — remove a single element from middle of
    /// an array. Josephus simulate uses this.
    #[test]
    fn splice_single_remove_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @circle = 0:5; splice @circle, 2, 1");
    }

    /// `atan2(0, -1)` for π — Monte Carlo's true-reference value.
    #[test]
    fn atan2_call_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn MC::true_pi = atan2(0, -1)");
        parse_ok("my $pi = atan2(0, -1)");
    }

    /// Flat 1-D array indexed as 2-D via `r * COLS + c` — Sudoku
    /// board layout. Arithmetic inside subscripts.
    #[test]
    fn flat_2d_array_indexing_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @board = (0) x 81; \
             my $r = 3; my $c = 5; \
             $board[$r * 9 + $c] = 7; \
             my $v = $board[$r * 9 + $c]",
        );
    }

    /// `par` is callable as a top-level expression, not just an
    /// `~>` thread-macro stage. Prefix form: `par { BLOCK } LIST`.
    /// (Previously parser only accepted `par` inside thread macros,
    /// emitting "Undefined subroutine &par" at runtime for any other
    /// call site.)
    #[test]
    fn par_top_level_prefix_form_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my @r = par { _ * 2 } (1, 2, 3, 4)");
        parse_ok("par { p _ } @big");
    }

    /// 2-D DP table with `max()` step — LCS / Levenshtein / Damerau /
    /// general edit-distance pattern. Validates that 3-way max +
    /// nested arrayref subscript chain parses cleanly.
    #[test]
    fn dp_max_step_chained_subscript_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @d; for my $i (0:3) { $d[$i] = [(0) x 4] } \
             $d[1][1] = max($d[0][1], $d[1][0]); \
             my $r = $d[1][1]",
        );
    }

    /// Rolling polynomial hash arithmetic — Rabin-Karp's window update.
    #[test]
    fn rolling_hash_arithmetic_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my $h = 0; my $base = 257; my $mod = 1000000007; my $high = 256; \
             my $drop = 65; my $add = 90; \
             $h = (($h - $drop * $high) * $base + $add) % $mod; \
             $h = ($h + $mod) % $mod",
        );
    }

    /// Triple-nested loop with index expressions on a 2-D arrayref —
    /// Floyd-Warshall's k/i/j signature.
    #[test]
    fn triple_nested_2d_via_k_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @d; for my $i (0:3) { $d[$i] = [(0) x 4] } \
             for my $k (0:3) { for my $i (0:3) { for my $j (0:3) { \
                $d[$i][$j] = $d[$i][$k] + $d[$k][$j] \
                    if $d[$i][$k] + $d[$k][$j] < $d[$i][$j] \
             } } }",
        );
    }

    /// DP fill with `@dp = ($INF) x ($amount + 1)` repeat-init.
    /// Coin-change shape.
    #[test]
    fn dp_array_repeat_init_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $amount = 11; my $INF = 1e18; my @dp = ($INF) x ($amount + 1); $dp[0] = 0");
    }

    /// `join("", rev split //, $s)` — palindrome check pipeline.
    #[test]
    fn rev_split_join_chain_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $s = \"abc\"; my $r = join(\"\", rev split //, $s)");
    }

    /// C-style `for` with explicit init / cond / decrement step —
    /// heap-sort's sift-down walks the heap children from end down.
    #[test]
    fn cstyle_for_decrement_with_arrayref_swap_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @arr = (1, 2, 3); my $r = \\@arr; \
             for (my $end = len(@arr) - 1; $end > 0; $end--) { \
                ($r->[0], $r->[$end]) = ($r->[$end], $r->[0]) \
             }",
        );
    }

    /// `shift @q` inside a while-loop driving a BFS-style queue —
    /// Kahn's topological sort.
    #[test]
    fn shift_in_while_loop_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @q = (0, 1, 2); my @out; \
             while (len(@q) > 0) { my $u = shift @q; push @out, $u }",
        );
    }

    /// Modular exponentiation by squaring — Miller-Rabin's core. While
    /// loop with `int($e / 2)`, modular multiply, and conditional update.
    #[test]
    fn mod_pow_squaring_loop_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn MR::mod_pow($base, $exp, $m) { \
                my $result = 1; my $bs = $base % $m; my $e = $exp; \
                while ($e > 0) { \
                    $result = ($result * $bs) % $m if $e % 2 == 1; \
                    $e = int($e / 2); \
                    $bs = ($bs * $bs) % $m \
                } \
                $result \
             }",
        );
    }

    /// 2-D DP traceback: walk back from dp[n][target], conditionally
    /// taking or skipping each item. Subset-sum reconstruct shape.
    #[test]
    fn dp_traceback_walk_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @xs = (1, 2, 3); my $n = 3; my $target = 4; \
             my @dp; for my $i (0:$n) { $dp[$i] = [(0) x ($target + 1)] } \
             my @out; my $j = $target; \
             for (my $i = $n; $i > 0; $i--) { \
                my $v = $xs[$i - 1]; \
                if ($j >= $v && $dp[$i - 1][$j - $v] == 1) { \
                    unshift @out, $v; $j -= $v \
                } \
             }",
        );
    }

    /// Binary search inside a `for` loop — LIS patience-sort variant
    /// using `tails` array. while-loop with `int(($lo+$hi)/2)`.
    #[test]
    fn binary_search_in_for_loop_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @xs = (3, 1, 4, 1, 5); my @tails; \
             for my $x (@xs) { \
                my $lo = 0; my $hi = len @tails; \
                while ($lo < $hi) { \
                    my $mid = int(($lo + $hi) / 2); \
                    if ($tails[$mid] < $x) { $lo = $mid + 1 } else { $hi = $mid } \
                } \
                if ($lo == len @tails) { push @tails, $x } else { $tails[$lo] = $x } \
             }",
        );
    }

    /// Edge-relaxation loop with destructure + early-skip — Bellman-Ford
    /// shape. Tests `my ($u, $w, $cost) = @$e` inside a for-loop.
    #[test]
    fn edge_relaxation_destructure_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @edges = ([0, 1, 5], [1, 2, -3]); \
             my @dist = (0, 1e18, 1e18); my $INF = 1e18; \
             for my $e (@edges) { \
                my ($u, $w, $cost) = @$e; \
                next if $dist[$u] >= $INF; \
                $dist[$w] = $dist[$u] + $cost if $dist[$u] + $cost < $dist[$w] \
             }",
        );
    }

    /// Bitwise `&` with negation: `$x & -$x` — Fenwick tree's
    /// lowest-set-bit isolation.
    #[test]
    fn bitwise_and_with_negation_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("fn Fenwick::lsb($x) = $x & -$x");
        parse_ok("my $k = 12; my $lo_bit = $k & -$k");
    }

    /// `@count[v] = old_v; running += c` — counting-sort cumulative
    /// transform. Tests serial assign-and-update inside a for loop.
    #[test]
    fn counting_sort_cumulative_loop_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @count = (3, 1, 2, 4); my $running = 0; \
             for my $v (0:3) { \
                my $c = $count[$v]; \
                $count[$v] = $running; \
                $running += $c \
             }",
        );
    }

    /// Recursive tree walk with hashref nodes — Huffman tree traversal
    /// pattern. Validates that `defined $node` + `exists $node->{sym}`
    /// + child-recursion all parse cleanly.
    #[test]
    fn recursive_tree_walk_with_hashref_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn Huff::walk($node, $prefix, $codes) { \
                return unless defined $node; \
                if (exists $node->{sym}) { \
                    $codes->{$node->{sym}} = $prefix eq \"\" ? \"0\" : $prefix; \
                    return \
                } \
                Huff::walk($node->{left},  $prefix . \"0\", $codes); \
                Huff::walk($node->{right}, $prefix . \"1\", $codes) \
             }",
        );
    }

    /// Z-array maintained-window arithmetic — three-way min via
    /// ternary, increment-while-match. Z-algorithm core.
    #[test]
    fn z_array_window_arithmetic_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @c = (\"a\", \"b\", \"a\"); my $n = 3; \
             my @z = (0) x $n; $z[0] = $n; \
             my $l = 0; my $r = 0; my $i = 1; \
             if ($i < $r) { \
                my $inside = $r - $i < $z[$i - $l] ? $r - $i : $z[$i - $l]; \
                $z[$i] = $inside \
             } \
             while ($i + $z[$i] < $n && $c[$z[$i]] eq $c[$i + $z[$i]]) { $z[$i]++ }",
        );
    }

    /// BFS expansion with parent-linked hashref nodes — A* /
    /// general pathfinding shape. Inner `for` over neighbor offsets.
    #[test]
    fn bfs_with_parent_link_node_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @open = (+{ r => 0, c => 0, g => 0, parent => undef }); \
             while (len(@open) > 0) { \
                my $cur = shift @open; \
                for my $d ([-1, 0], [1, 0], [0, -1], [0, 1]) { \
                    my ($dr, $dc) = @$d; \
                    push @open, +{ \
                        r => $cur->{r} + $dr, c => $cur->{c} + $dc, \
                        g => $cur->{g} + 1, parent => $cur \
                    } \
                } \
                last \
             }",
        );
    }

    /// Cross-product sort comparator with collinear tiebreak —
    /// Graham scan's polar sort.
    #[test]
    fn cross_product_sort_comparator_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn Hull::cross($p1, $p2, $p3) = \
                ($p2->[0] - $p1->[0]) * ($p3->[1] - $p1->[1]) - \
                ($p2->[1] - $p1->[1]) * ($p3->[0] - $p1->[0]); \
             my $pivot = [0, 0]; my @pts = ([1, 1], [2, 0]); \
             my @sorted = sort { \
                my $c = Hull::cross($pivot, _0, _1); \
                $c == 0 ? 0 : ($c < 0 ? 1 : -1) \
             } @pts",
        );
    }

    /// Lomuto partition + recursive bisection — quickselect's loop.
    /// Inner loop with `$i++` + swap on each match.
    #[test]
    fn lomuto_partition_loop_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn QS::partition($arr, $lo, $hi) { \
                my $pivot = $arr->[$hi]; \
                my $i = $lo - 1; \
                for my $j ($lo:$hi - 1) { \
                    if ($arr->[$j] <= $pivot) { \
                        $i++; \
                        ($arr->[$i], $arr->[$j]) = ($arr->[$j], $arr->[$i]) \
                    } \
                } \
                ($arr->[$i + 1], $arr->[$hi]) = ($arr->[$hi], $arr->[$i + 1]); \
                $i + 1 \
             }",
        );
    }

    /// `do { x } while (cond)` shape with diff-then-gcd — Pollard rho's
    /// tortoise-and-hare loop. Tests absolute-difference via ternary.
    #[test]
    fn tortoise_hare_diff_loop_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my $x = 2; my $y = 2; my $n = 35; my $d = 1; \
             while ($d == 1) { \
                $x = ($x * $x + 1) % $n; \
                $y = ($y * $y + 1) % $n; \
                $y = ($y * $y + 1) % $n; \
                my $diff = $x > $y ? $x - $y : $y - $x; \
                $d = $diff \
             }",
        );
    }

    /// Recursive ext_gcd returning a 3-tuple via arrayref destructure.
    /// Modular-inverse pattern.
    #[test]
    fn ext_gcd_recursive_destructure_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn Mod::ext_gcd($va, $vb) { \
                return [$va, 1, 0] if $vb == 0; \
                my $r = Mod::ext_gcd($vb, $va % $vb); \
                my ($g, $x1, $y1) = @$r; \
                [$g, $y1, $x1 - int($va / $vb) * $y1] \
             }",
        );
    }

    /// Convolution-recurrence DP — Catalan number computation.
    /// Inner accumulator with mult on each iteration.
    #[test]
    fn convolution_recurrence_dp_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "my @c = (1); my $n = 5; \
             for my $i (1:$n) { \
                my $sum = 0; \
                for my $j (0:$i - 1) { $sum += $c[$j] * $c[$i - 1 - $j] } \
                push @c, $sum \
             }",
        );
    }

    /// Tarjan SCC state: shared mutable hashref carries the entire
    /// algorithm's bookkeeping (`index`, `idx_of`, `low_of`,
    /// `on_stack`, `stack`, `sccs`) — passed by reference to the
    /// recursive worker.
    #[test]
    fn tarjan_scc_shared_state_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn SCC::strong_connect($s, $v) { \
                $s->{idx_of}{$v} = $s->{index}; \
                $s->{low_of}{$v} = $s->{index}; \
                $s->{index}++; \
                push @{$s->{stack}}, $v; \
                $s->{on_stack}{$v} = 1; \
                for my $w (@{$s->{adj}{$v}}) { \
                    if (!exists $s->{idx_of}{$w}) { \
                        SCC::strong_connect($s, $w); \
                        $s->{low_of}{$v} = $s->{low_of}{$w} if $s->{low_of}{$w} < $s->{low_of}{$v} \
                    } \
                } \
             }",
        );
    }

    /// Heap's algorithm permutation generator — recursive in-place
    /// swap with parity-conditional pivot choice.
    #[test]
    fn heaps_algorithm_recursive_parses() {
        let _g = NoInteropGuard::on();
        parse_ok(
            "fn Perm::heaps_inner($arr, $n, $out) { \
                if ($n == 1) { my @snap = @$arr; push @$out, \\@snap; return } \
                for my $i (0:$n - 1) { \
                    Perm::heaps_inner($arr, $n - 1, $out); \
                    if ($n % 2 == 0) { \
                        ($arr->[$i], $arr->[$n - 1]) = ($arr->[$n - 1], $arr->[$i]) \
                    } else { \
                        ($arr->[0], $arr->[$n - 1]) = ($arr->[$n - 1], $arr->[0]) \
                    } \
                } \
             }",
        );
    }

    /// `format` is a Perl FORMAT-declaration keyword. The lexer must
    /// NOT eat `format` when it appears as a hash key
    /// (`$h{format}`, `{format => ...}`), a method name
    /// (`$obj->format`), a namespaced tail (`Foo::format`), or a
    /// list/expr item with terminator follow-up. Previously
    /// `$opts{format} = "csv"` triggered "Expected '=' after format
    /// name" because the lexer greedily entered format-decl mode.
    #[test]
    fn format_as_hash_key_parses() {
        parse_ok("my %opts; $opts{format} = \"csv\"");
        parse_ok("my %opts = (format => \"csv\", level => 9)");
        parse_ok("my $h = +{ format => \"csv\" }");
        parse_ok("my @keys = ($h->{format}, $h->{level})");
    }

    /// `format` after `->` is a method name, not the format keyword.
    #[test]
    fn format_as_method_call_parses() {
        parse_ok("class Foo { val: Str; fn format($self) { \"x\" } } my $f = Foo(val => \"y\"); my $s = $f->format()");
    }

    /// `format` after `::` is a namespaced fn name tail.
    #[test]
    fn format_as_namespaced_tail_parses() {
        parse_ok("fn Foo::format($x) = $x . \"!\"");
        parse_ok("fn Foo::format($x) = $x . \"!\"; my $r = Foo::format(\"hi\")");
    }

    /// Compound-assign on hash arrow-deref leaves the new value on the
    /// stack (uses `SetArrowHashKeep`, not `SetArrowHash`). Previously
    /// the no-keep variant left nothing for the statement-level Pop,
    /// which then ate a slot from the CALLER's stack frame — corrupting
    /// `dec($h) + dec($h) + dec($h)`-style multi-call expressions.
    /// See tests/suite/hashref_assignment_pin.rs for runtime pins.
    #[test]
    fn arrow_hash_compound_assign_parses_all_ops() {
        let _g = NoInteropGuard::on();
        parse_ok("my $h = +{n=>10}; $h->{n} -= 1");
        parse_ok("my $h = +{n=>10}; $h->{n} += 1");
        parse_ok("my $h = +{n=>10}; $h->{n} *= 2");
        parse_ok("my $h = +{n=>10}; $h->{n} /= 2");
        parse_ok("my $h = +{n=>10}; $h->{n} %= 3");
        parse_ok("my $h = +{n=>\"x\"}; $h->{n} .= \"y\"");
    }

    /// The compound-assign yields the NEW value as its expression
    /// result, same as plain `$h->{k} = v` does. Validated at parse
    /// time by accepting the use-as-rvalue shape `my $v = $h->{n} -= 1`.
    #[test]
    fn arrow_hash_compound_assign_value_chains_parses() {
        let _g = NoInteropGuard::on();
        parse_ok("my $h = +{n=>10}; my $v = $h->{n} -= 1");
        parse_ok("my $h = +{n=>10}; my @list = ($h->{n} += 5, $h->{n} += 5)");
        parse_ok("my $h = +{n=>10}; my $double = ($h->{n} -= 1) * 2");
    }
}
