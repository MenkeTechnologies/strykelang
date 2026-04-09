use crate::ast::*;
use crate::error::{PerlError, PerlResult};
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

    fn advance(&mut self) -> (Token, usize) {
        let tok = self.tokens.get(self.pos).cloned().unwrap_or((Token::Eof, 0));
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
            if self.eat(&Token::Semicolon) {
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

        let stmt = match self.peek().clone() {
            Token::Ident(ref kw) => match kw.as_str() {
                "if" => self.parse_if()?,
                "unless" => self.parse_unless()?,
                "while" => {
                    let mut s = self.parse_while()?;
                    if let StmtKind::While { label: ref mut lbl, .. } = s.kind {
                        *lbl = label;
                    }
                    s
                }
                "until" => {
                    let mut s = self.parse_until()?;
                    if let StmtKind::Until { label: ref mut lbl, .. } = s.kind {
                        *lbl = label;
                    }
                    s
                }
                "for" => {
                    let mut s = self.parse_for_or_foreach()?;
                    match s.kind {
                        StmtKind::For { label: ref mut lbl, .. }
                        | StmtKind::Foreach { label: ref mut lbl, .. } => *lbl = label,
                        _ => {}
                    }
                    s
                }
                "foreach" => {
                    let mut s = self.parse_foreach()?;
                    if let StmtKind::Foreach { label: ref mut lbl, .. } = s.kind {
                        *lbl = label;
                    }
                    s
                }
                "sub" => self.parse_sub_decl()?,
                "my" => self.parse_my_our_local("my")?,
                "our" => self.parse_my_our_local("our")?,
                "local" => self.parse_my_our_local("local")?,
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
                    let stmt = Statement { kind: StmtKind::Last(lbl.or(label)), line };
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
                    let stmt = Statement { kind: StmtKind::Next(lbl.or(label)), line };
                    self.parse_stmt_postfix_modifier(stmt)?
                }
                "redo" => {
                    self.advance();
                    self.eat(&Token::Semicolon);
                    Statement { kind: StmtKind::Redo(label), line }
                }
                "BEGIN" => {
                    self.advance();
                    let block = self.parse_block()?;
                    Statement { kind: StmtKind::Begin(block), line }
                }
                "END" => {
                    self.advance();
                    let block = self.parse_block()?;
                    Statement { kind: StmtKind::End(block), line }
                }
                _ => {
                    let expr = self.parse_expression()?;
                    let stmt = self.maybe_postfix_modifier(expr)?;
                    self.eat(&Token::Semicolon);
                    stmt
                }
            },
            Token::LBrace => {
                let block = self.parse_block()?;
                Statement { kind: StmtKind::Block(block), line }
            }
            _ => {
                let expr = self.parse_expression()?;
                let stmt = self.maybe_postfix_modifier(expr)?;
                self.eat(&Token::Semicolon);
                stmt
            }
        };

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
                _ => Ok(Statement { kind: StmtKind::Expression(expr), line }),
            },
            _ => Ok(Statement { kind: StmtKind::Expression(expr), line }),
        }
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

    fn parse_if(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'if'
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;

        let mut elsifs = Vec::new();
        let mut else_block = None;

        loop {
            if let Token::Ident(ref kw) = self.peek().clone() {
                if kw == "elsif" {
                    self.advance();
                    self.expect(&Token::LParen)?;
                    let c = self.parse_expression()?;
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
            kind: StmtKind::If { condition: cond, body, elsifs, else_block },
            line,
        })
    }

    fn parse_unless(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'unless'
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
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
            kind: StmtKind::Unless { condition: cond, body, else_block },
            line,
        })
    }

    fn parse_while(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'while'
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        Ok(Statement {
            kind: StmtKind::While { condition: cond, body, label: None },
            line,
        })
    }

    fn parse_until(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'until'
        self.expect(&Token::LParen)?;
        let cond = self.parse_expression()?;
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        Ok(Statement {
            kind: StmtKind::Until { condition: cond, body, label: None },
            line,
        })
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
                    Ok(Statement {
                        kind: StmtKind::Foreach {
                            var: "_".to_string(),
                            list,
                            body,
                            label: None,
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
                Ok(Statement {
                    kind: StmtKind::Foreach { var, list, body, label: None },
                    line,
                })
            }
            Token::ScalarVar(_) => {
                let var = self.parse_scalar_var_name()?;
                self.expect(&Token::LParen)?;
                let list = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                let body = self.parse_block()?;
                Ok(Statement {
                    kind: StmtKind::Foreach { var, list, body, label: None },
                    line,
                })
            }
            _ => {
                self.parse_c_style_for(line)
            }
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
        let condition = if matches!(self.peek(), Token::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect(&Token::Semicolon)?;
        let step = if matches!(self.peek(), Token::RParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect(&Token::RParen)?;
        let body = self.parse_block()?;
        Ok(Statement {
            kind: StmtKind::For { init, condition, step, body, label: None },
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
        Ok(Statement {
            kind: StmtKind::Foreach { var, list, body, label: None },
            line,
        })
    }

    fn parse_scalar_var_name(&mut self) -> PerlResult<String> {
        match self.advance() {
            (Token::ScalarVar(name), _) => Ok(name),
            (tok, line) => Err(PerlError::syntax(format!("Expected scalar variable, got {:?}", tok), line)),
        }
    }

    fn parse_sub_decl(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'sub'
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => return Err(PerlError::syntax(format!("Expected sub name, got {:?}", tok), line)),
        };
        // Optional prototype — skip it
        if matches!(self.peek(), Token::LParen) {
            self.advance();
            while !matches!(self.peek(), Token::RParen | Token::Eof) {
                self.advance();
            }
            self.expect(&Token::RParen)?;
        }
        let body = self.parse_block()?;
        Ok(Statement {
            kind: StmtKind::SubDecl { name, params: vec![], body },
            line,
        })
    }

    fn parse_my_our_local(&mut self, keyword: &str) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'my'/'our'/'local'

        let mut decls = Vec::new();

        if self.eat(&Token::LParen) {
            // my ($a, @b, %c)
            while !matches!(self.peek(), Token::RParen | Token::Eof) {
                let decl = self.parse_var_decl()?;
                decls.push(decl);
                if !self.eat(&Token::Comma) {
                    break;
                }
            }
            self.expect(&Token::RParen)?;
        } else {
            decls.push(self.parse_var_decl()?);
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
            "our" => StmtKind::Our(decls),
            "local" => StmtKind::Local(decls),
            _ => unreachable!(),
        };
        Ok(Statement { kind, line })
    }

    fn parse_var_decl(&mut self) -> PerlResult<VarDecl> {
        match self.advance() {
            (Token::ScalarVar(name), _) => Ok(VarDecl {
                sigil: Sigil::Scalar,
                name,
                initializer: None,
            }),
            (Token::ArrayVar(name), _) => Ok(VarDecl {
                sigil: Sigil::Array,
                name,
                initializer: None,
            }),
            (Token::HashVar(name), _) => Ok(VarDecl {
                sigil: Sigil::Hash,
                name,
                initializer: None,
            }),
            (tok, line) => Err(PerlError::syntax(
                format!("Expected variable in declaration, got {:?}", tok),
                line,
            )),
        }
    }

    fn parse_package(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'package'
        let name = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => return Err(PerlError::syntax(format!("Expected package name, got {:?}", tok), line)),
        };
        // Handle Foo::Bar
        let mut full_name = name;
        while self.eat(&Token::PackageSep) {
            if let (Token::Ident(part), _) = self.advance() {
                full_name = format!("{}::{}", full_name, part);
            }
        }
        self.eat(&Token::Semicolon);
        Ok(Statement { kind: StmtKind::Package { name: full_name }, line })
    }

    fn parse_use(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'use'
        let module = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => return Err(PerlError::syntax(format!("Expected module name after use, got {:?}", tok), line)),
        };
        let mut full_name = module;
        while self.eat(&Token::PackageSep) {
            if let (Token::Ident(part), _) = self.advance() {
                full_name = format!("{}::{}", full_name, part);
            }
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
            kind: StmtKind::Use { module: full_name, imports },
            line,
        })
    }

    fn parse_no(&mut self) -> PerlResult<Statement> {
        let line = self.peek_line();
        self.advance(); // 'no'
        let module = match self.advance() {
            (Token::Ident(n), _) => n,
            (tok, line) => return Err(PerlError::syntax(format!("Expected module name after no, got {:?}", tok), line)),
        };
        let mut full_name = module;
        while self.eat(&Token::PackageSep) {
            if let (Token::Ident(part), _) = self.advance() {
                full_name = format!("{}::{}", full_name, part);
            }
        }
        self.eat(&Token::Semicolon);
        Ok(Statement {
            kind: StmtKind::No { module: full_name, imports: vec![] },
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
        let stmt = Statement { kind: StmtKind::Return(val), line };
        if let Token::Ident(ref kw) = self.peek().clone() {
            match kw.as_str() {
                "if" => {
                    self.advance();
                    let cond = self.parse_expression()?;
                    self.eat(&Token::Semicolon);
                    return Ok(Statement {
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
        Ok(Expr { kind: ExprKind::List(exprs), line })
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
            Token::PlusAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Add, value: Box::new(r) }, line }) }
            Token::MinusAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Sub, value: Box::new(r) }, line }) }
            Token::MulAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Mul, value: Box::new(r) }, line }) }
            Token::DivAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Div, value: Box::new(r) }, line }) }
            Token::ModAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Mod, value: Box::new(r) }, line }) }
            Token::PowAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Pow, value: Box::new(r) }, line }) }
            Token::DotAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::Concat, value: Box::new(r) }, line }) }
            Token::DefinedOrAssign => { self.advance(); let r = self.parse_assign_expr()?; Ok(Expr { kind: ExprKind::CompoundAssign { target: Box::new(expr), op: BinOp::DefinedOr, value: Box::new(r) }, line }) }
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
                kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::LogOrWord, right: Box::new(right) },
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
                kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::LogAndWord, right: Box::new(right) },
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
                kind: ExprKind::UnaryOp { op: UnaryOp::LogNotWord, expr: Box::new(expr) },
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
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op, right: Box::new(right) }, line };
        }
        Ok(left)
    }

    fn parse_log_and(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_bit_or()?;
        while matches!(self.peek(), Token::LogAnd) {
            let line = left.line;
            self.advance();
            let right = self.parse_bit_or()?;
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::LogAnd, right: Box::new(right) }, line };
        }
        Ok(left)
    }

    fn parse_bit_or(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_bit_xor()?;
        while matches!(self.peek(), Token::BitOr) {
            let line = left.line;
            self.advance();
            let right = self.parse_bit_xor()?;
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::BitOr, right: Box::new(right) }, line };
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_bit_and()?;
        while matches!(self.peek(), Token::BitXor) {
            let line = left.line;
            self.advance();
            let right = self.parse_bit_and()?;
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::BitXor, right: Box::new(right) }, line };
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> PerlResult<Expr> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), Token::BitAnd) {
            let line = left.line;
            self.advance();
            let right = self.parse_equality()?;
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::BitAnd, right: Box::new(right) }, line };
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
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op, right: Box::new(right) }, line };
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
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op, right: Box::new(right) }, line };
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
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op, right: Box::new(right) }, line };
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
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op, right: Box::new(right) }, line };
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
                        kind: ExprKind::Repeat { expr: Box::new(left), count: Box::new(right) },
                        line,
                    };
                    continue;
                }
                _ => break,
            };
            let line = left.line;
            self.advance();
            let right = self.parse_regex_bind()?;
            left = Expr { kind: ExprKind::BinOp { left: Box::new(left), op, right: Box::new(right) }, line };
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
                            kind: ExprKind::Match { expr: Box::new(left), pattern, flags },
                            line,
                        })
                    }
                    Token::Ident(ref s) if s.starts_with('\x00') => {
                        let (Token::Ident(encoded), _) = self.advance() else { unreachable!() };
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
                    _ => Err(PerlError::syntax("Expected regex after =~", line)),
                }
            }
            Token::BindNotMatch => {
                let line = left.line;
                self.advance();
                match self.advance() {
                    (Token::Regex(pattern, flags), _) => Ok(Expr {
                        kind: ExprKind::UnaryOp {
                            op: UnaryOp::LogNot,
                            expr: Box::new(Expr {
                                kind: ExprKind::Match { expr: Box::new(left), pattern, flags },
                                line,
                            }),
                        },
                        line,
                    }),
                    (_, line) => Err(PerlError::syntax("Expected regex after !~", line)),
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
                kind: ExprKind::Range { from: Box::new(left), to: Box::new(right) },
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
                Ok(Expr { kind: ExprKind::UnaryOp { op: UnaryOp::Negate, expr: Box::new(expr) }, line })
            }
            Token::LogNot => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr { kind: ExprKind::UnaryOp { op: UnaryOp::LogNot, expr: Box::new(expr) }, line })
            }
            Token::BitNot => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr { kind: ExprKind::UnaryOp { op: UnaryOp::BitNot, expr: Box::new(expr) }, line })
            }
            Token::Increment => {
                self.advance();
                let expr = self.parse_postfix()?;
                Ok(Expr { kind: ExprKind::UnaryOp { op: UnaryOp::PreIncrement, expr: Box::new(expr) }, line })
            }
            Token::Decrement => {
                self.advance();
                let expr = self.parse_postfix()?;
                Ok(Expr { kind: ExprKind::UnaryOp { op: UnaryOp::PreDecrement, expr: Box::new(expr) }, line })
            }
            Token::Backslash => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr { kind: ExprKind::ScalarRef(Box::new(expr)), line })
            }
            Token::FileTest(op) => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr { kind: ExprKind::FileTest { op, expr: Box::new(expr) }, line })
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
                kind: ExprKind::BinOp { left: Box::new(left), op: BinOp::Pow, right: Box::new(right) },
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
                    expr = Expr { kind: ExprKind::PostfixOp { expr: Box::new(expr), op: PostfixOp::Increment }, line };
                }
                Token::Decrement => {
                    let line = expr.line;
                    self.advance();
                    expr = Expr { kind: ExprKind::PostfixOp { expr: Box::new(expr), op: PostfixOp::Decrement }, line };
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
                                kind: ExprKind::ArrowDeref { expr: Box::new(expr), index: Box::new(index), kind: DerefKind::Array },
                                line,
                            };
                        }
                        Token::LBrace => {
                            self.advance();
                            let key = self.parse_expression()?;
                            self.expect(&Token::RBrace)?;
                            expr = Expr {
                                kind: ExprKind::ArrowDeref { expr: Box::new(expr), index: Box::new(key), kind: DerefKind::Hash },
                                line,
                            };
                        }
                        Token::LParen => {
                            self.advance();
                            let args = self.parse_arg_list()?;
                            self.expect(&Token::RParen)?;
                            expr = Expr {
                                kind: ExprKind::ArrowDeref { expr: Box::new(expr), index: Box::new(Expr { kind: ExprKind::List(args), line }), kind: DerefKind::Call },
                                line,
                            };
                        }
                        Token::Ident(method) => {
                            self.advance();
                            let args = if self.eat(&Token::LParen) {
                                let a = self.parse_arg_list()?;
                                self.expect(&Token::RParen)?;
                                a
                            } else {
                                vec![]
                            };
                            expr = Expr {
                                kind: ExprKind::MethodCall { object: Box::new(expr), method, args },
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
                        expr = Expr { kind: ExprKind::ArrayElement { array: name, index: Box::new(index) }, line };
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
                        expr = Expr { kind: ExprKind::HashElement { hash: name, key: Box::new(key) }, line };
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
            Token::Integer(n) => { self.advance(); Ok(Expr { kind: ExprKind::Integer(n), line }) }
            Token::Float(f) => { self.advance(); Ok(Expr { kind: ExprKind::Float(f), line }) }
            Token::SingleString(s) => { self.advance(); Ok(Expr { kind: ExprKind::String(s), line }) }
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
                Ok(Expr { kind: ExprKind::Regex(pattern, flags), line })
            }
            Token::QW(words) => {
                self.advance();
                Ok(Expr { kind: ExprKind::QW(words), line })
            }
            Token::ScalarVar(name) => {
                self.advance();
                Ok(Expr { kind: ExprKind::ScalarVar(name), line })
            }
            Token::ArrayVar(name) => {
                self.advance();
                // Check for slice: @arr[...] or @hash{...}
                match self.peek() {
                    Token::LBracket => {
                        self.advance();
                        let indices = self.parse_arg_list()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Expr { kind: ExprKind::ArraySlice { array: name, indices }, line })
                    }
                    _ => Ok(Expr { kind: ExprKind::ArrayVar(name), line }),
                }
            }
            Token::HashVar(name) => {
                self.advance();
                Ok(Expr { kind: ExprKind::HashVar(name), line })
            }
            Token::LParen => {
                self.advance();
                if matches!(self.peek(), Token::RParen) {
                    self.advance();
                    return Ok(Expr { kind: ExprKind::List(vec![]), line });
                }
                let expr = self.parse_expression()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::LBracket => {
                self.advance();
                let elems = self.parse_arg_list()?;
                self.expect(&Token::RBracket)?;
                Ok(Expr { kind: ExprKind::ArrayRef(elems), line })
            }
            Token::LBrace => {
                // Could be hash ref or block — disambiguate
                self.advance();
                // Try to parse as hash ref: { key => val, ... }
                let saved = self.pos;
                match self.try_parse_hash_ref() {
                    Ok(pairs) => Ok(Expr { kind: ExprKind::HashRef(pairs), line }),
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
                        Ok(Expr { kind: ExprKind::CodeRef { params: vec![], body: stmts }, line })
                    }
                }
            }
            Token::Diamond => {
                self.advance();
                Ok(Expr { kind: ExprKind::ReadLine(None), line })
            }
            Token::ReadLine(handle) => {
                self.advance();
                Ok(Expr { kind: ExprKind::ReadLine(Some(handle)), line })
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
                                expr: Box::new(Expr { kind: ExprKind::ScalarVar("_".into()), line }),
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
                                expr: Box::new(Expr { kind: ExprKind::ScalarVar("_".into()), line }),
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

            tok => Err(PerlError::syntax(format!("Unexpected token {:?}", tok), line)),
        }
    }

    fn parse_named_expr(&mut self, name: String) -> PerlResult<Expr> {
        let line = self.peek_line();
        self.advance(); // consume the ident

        match name.as_str() {
            "print" => self.parse_print_like(|h, a| ExprKind::Print { handle: h, args: a }),
            "say" => self.parse_print_like(|h, a| ExprKind::Say { handle: h, args: a }),
            "printf" => self.parse_print_like(|h, a| ExprKind::Printf { handle: h, args: a }),
            "die" => {
                let args = self.parse_list_until_terminator()?;
                Ok(Expr { kind: ExprKind::Die(args), line })
            }
            "warn" => {
                let args = self.parse_list_until_terminator()?;
                Ok(Expr { kind: ExprKind::Warn(args), line })
            }
            "chomp" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Chomp(Box::new(a)), line }) }
            "chop" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Chop(Box::new(a)), line }) }
            "length" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Length(Box::new(a)), line }) }
            "defined" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Defined(Box::new(a)), line }) }
            "ref" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Ref(Box::new(a)), line }) }
            "undef" => {
                if matches!(self.peek(), Token::ScalarVar(_) | Token::ArrayVar(_) | Token::HashVar(_)) {
                    let _ = self.advance();
                }
                Ok(Expr { kind: ExprKind::Undef, line })
            }
            "scalar" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::ScalarContext(Box::new(a)), line }) }
            "abs" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Abs(Box::new(a)), line }) }
            "int" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Int(Box::new(a)), line }) }
            "sqrt" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Sqrt(Box::new(a)), line }) }
            "hex" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Hex(Box::new(a)), line }) }
            "oct" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Oct(Box::new(a)), line }) }
            "chr" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Chr(Box::new(a)), line }) }
            "ord" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Ord(Box::new(a)), line }) }
            "lc" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Lc(Box::new(a)), line }) }
            "uc" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Uc(Box::new(a)), line }) }
            "lcfirst" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Lcfirst(Box::new(a)), line }) }
            "ucfirst" => { let a = self.parse_one_arg_or_default()?; Ok(Expr { kind: ExprKind::Ucfirst(Box::new(a)), line }) }
            "push" => {
                let args = self.parse_builtin_args()?;
                let (first, rest) = args.split_first().ok_or_else(|| PerlError::syntax("push requires arguments", line))?;
                Ok(Expr { kind: ExprKind::Push { array: Box::new(first.clone()), values: rest.to_vec() }, line })
            }
            "pop" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Pop(Box::new(a)), line }) }
            "shift" => { let a = self.parse_one_arg_or_argv()?; Ok(Expr { kind: ExprKind::Shift(Box::new(a)), line }) }
            "unshift" => {
                let args = self.parse_builtin_args()?;
                let (first, rest) = args.split_first().ok_or_else(|| PerlError::syntax("unshift requires arguments", line))?;
                Ok(Expr { kind: ExprKind::Unshift { array: Box::new(first.clone()), values: rest.to_vec() }, line })
            }
            "splice" => {
                let args = self.parse_builtin_args()?;
                let mut iter = args.into_iter();
                let array = Box::new(iter.next().ok_or_else(|| PerlError::syntax("splice requires arguments", line))?);
                let offset = iter.next().map(Box::new);
                let length = iter.next().map(Box::new);
                let replacement: Vec<Expr> = iter.collect();
                Ok(Expr { kind: ExprKind::Splice { array, offset, length, replacement }, line })
            }
            "delete" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Delete(Box::new(a)), line }) }
            "exists" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Exists(Box::new(a)), line }) }
            "keys" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Keys(Box::new(a)), line }) }
            "values" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Values(Box::new(a)), line }) }
            "each" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Each(Box::new(a)), line }) }
            "reverse" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::ReverseExpr(Box::new(a)), line }) }
            "join" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(PerlError::syntax("join requires separator and list", line));
                }
                Ok(Expr {
                    kind: ExprKind::JoinExpr {
                        separator: Box::new(args[0].clone()),
                        list: Box::new(Expr { kind: ExprKind::List(args[1..].to_vec()), line }),
                    },
                    line,
                })
            }
            "split" => {
                let args = self.parse_builtin_args()?;
                let pattern = args.first().cloned().unwrap_or(Expr { kind: ExprKind::String(" ".into()), line });
                let string = args.get(1).cloned().unwrap_or(Expr { kind: ExprKind::ScalarVar("_".into()), line });
                let limit = args.get(2).cloned().map(Box::new);
                Ok(Expr { kind: ExprKind::SplitExpr { pattern: Box::new(pattern), string: Box::new(string), limit }, line })
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
                let (first, rest) = args.split_first().ok_or_else(|| PerlError::syntax("sprintf requires format", line))?;
                Ok(Expr { kind: ExprKind::Sprintf { format: Box::new(first.clone()), args: rest.to_vec() }, line })
            }
            "map" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr { kind: ExprKind::MapExpr { block, list: Box::new(list) }, line })
            }
            "grep" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr { kind: ExprKind::GrepExpr { block, list: Box::new(list) }, line })
            }
            "sort" => {
                // sort may have optional cmp block
                if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
                    let list = if self.eat(&Token::Comma) {
                        self.parse_expression()?
                    } else {
                        self.parse_expression()?
                    };
                    Ok(Expr { kind: ExprKind::SortExpr { cmp: Some(block), list: Box::new(list) }, line })
                } else {
                    let list = self.parse_expression()?;
                    Ok(Expr { kind: ExprKind::SortExpr { cmp: None, list: Box::new(list) }, line })
                }
            }
            // Parallel extensions
            "pmap" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr { kind: ExprKind::PMapExpr { block, list: Box::new(list) }, line })
            }
            "pgrep" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr { kind: ExprKind::PGrepExpr { block, list: Box::new(list) }, line })
            }
            "pfor" => {
                let (block, list) = self.parse_block_list()?;
                Ok(Expr { kind: ExprKind::PForExpr { block, list: Box::new(list) }, line })
            }
            "psort" => {
                if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
                    let list = self.parse_expression()?;
                    Ok(Expr { kind: ExprKind::PSortExpr { cmp: Some(block), list: Box::new(list) }, line })
                } else {
                    let list = self.parse_expression()?;
                    Ok(Expr { kind: ExprKind::PSortExpr { cmp: None, list: Box::new(list) }, line })
                }
            }
            "open" => {
                let args = self.parse_builtin_args()?;
                if args.len() < 2 {
                    return Err(PerlError::syntax("open requires at least 2 arguments", line));
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
            "close" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Close(Box::new(a)), line }) }
            "eof" => {
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    if matches!(self.peek(), Token::RParen) {
                        self.advance();
                        Ok(Expr { kind: ExprKind::Eof(None), line })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr { kind: ExprKind::Eof(Some(Box::new(a))), line })
                    }
                } else {
                    Ok(Expr { kind: ExprKind::Eof(None), line })
                }
            }
            "system" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr { kind: ExprKind::System(args), line })
            }
            "exec" => {
                let args = self.parse_builtin_args()?;
                Ok(Expr { kind: ExprKind::Exec(args), line })
            }
            "eval" => {
                let a = if matches!(self.peek(), Token::LBrace) {
                    let block = self.parse_block()?;
                    Expr { kind: ExprKind::CodeRef { params: vec![], body: block }, line }
                } else {
                    self.parse_one_arg()?
                };
                Ok(Expr { kind: ExprKind::Eval(Box::new(a)), line })
            }
            "do" => {
                let a = self.parse_one_arg()?;
                Ok(Expr { kind: ExprKind::Do(Box::new(a)), line })
            }
            "require" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Require(Box::new(a)), line }) }
            "exit" => {
                if matches!(self.peek(), Token::Semicolon | Token::RBrace | Token::Eof) {
                    Ok(Expr { kind: ExprKind::Exit(None), line })
                } else {
                    let a = self.parse_one_arg()?;
                    Ok(Expr { kind: ExprKind::Exit(Some(Box::new(a))), line })
                }
            }
            "chdir" => { let a = self.parse_one_arg()?; Ok(Expr { kind: ExprKind::Chdir(Box::new(a)), line }) }
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
                Ok(Expr { kind: ExprKind::Unlink(args), line })
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
                        Ok(Expr { kind: ExprKind::Caller(None), line })
                    } else {
                        let a = self.parse_expression()?;
                        self.expect(&Token::RParen)?;
                        Ok(Expr { kind: ExprKind::Caller(Some(Box::new(a))), line })
                    }
                } else {
                    Ok(Expr { kind: ExprKind::Caller(None), line })
                }
            }
            "wantarray" => Ok(Expr { kind: ExprKind::Wantarray, line }),
            "sub" => {
                // Anonymous sub
                let body = self.parse_block()?;
                Ok(Expr { kind: ExprKind::CodeRef { params: vec![], body }, line })
            }
            _ => {
                // Generic function call
                // Check for fat arrow (bareword string in hash)
                if matches!(self.peek(), Token::FatArrow) {
                    return Ok(Expr { kind: ExprKind::String(name), line });
                }
                // Function call with optional parens
                if matches!(self.peek(), Token::LParen) {
                    self.advance();
                    let args = self.parse_arg_list()?;
                    self.expect(&Token::RParen)?;
                    Ok(Expr { kind: ExprKind::FuncCall { name, args }, line })
                } else if self.peek().is_term_start() {
                    // Perl allows func arg without parens
                    let args = self.parse_list_until_terminator()?;
                    Ok(Expr { kind: ExprKind::FuncCall { name, args }, line })
                } else {
                    // Bareword — treat as string (like hash key)
                    Ok(Expr { kind: ExprKind::String(name), line })
                }
            }
        }
    }

    fn parse_print_like(&mut self, make: impl FnOnce(Option<String>, Vec<Expr>) -> ExprKind) -> PerlResult<Expr> {
        let line = self.peek_line();
        // Check for filehandle: print STDERR "msg"
        let handle = if let Token::Ident(ref h) = self.peek().clone() {
            if h.chars().all(|c| c.is_uppercase() || c == '_') && !matches!(self.peek(), Token::LParen) {
                let h = h.clone();
                let saved = self.pos;
                self.advance();
                // Verify next token is a term start (not operator)
                if self.peek().is_term_start() || matches!(self.peek(), Token::DoubleString(_) | Token::SingleString(_)) {
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
        Ok(Expr { kind: make(handle, args), line })
    }

    fn parse_block_list(&mut self) -> PerlResult<(Block, Expr)> {
        let block = self.parse_block()?;
        self.eat(&Token::Comma);
        let list = self.parse_expression()?;
        Ok((block, list))
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
            Ok(Expr { kind: ExprKind::ScalarVar("_".into()), line: self.peek_line() })
        } else {
            self.parse_one_arg()
        }
    }

    fn parse_one_arg_or_argv(&mut self) -> PerlResult<Expr> {
        if matches!(
            self.peek(),
            Token::Semicolon | Token::RBrace | Token::RParen | Token::Eof | Token::Comma
        ) {
            Ok(Expr { kind: ExprKind::ArrayVar("_".into()), line: self.peek_line() })
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
        while !matches!(self.peek(), Token::RParen | Token::RBracket | Token::RBrace | Token::Eof) {
            args.push(self.parse_assign_expr()?);
            if !self.eat(&Token::Comma) && !self.eat(&Token::FatArrow) {
                break;
            }
        }
        Ok(args)
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
            // Check for postfix modifiers
            if let Token::Ident(ref kw) = self.peek().clone() {
                if matches!(kw.as_str(), "if" | "unless" | "while" | "until" | "for" | "foreach") && !args.is_empty() {
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
                            if chars[i] == '{' { depth += 1; }
                            else if chars[i] == '}' { depth -= 1; if depth == 0 { break; } }
                            key.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() { i += 1; } // skip }
                        // Build a HashElement expression for the interpreter
                        let key_expr = if key.starts_with('$') {
                            Expr { kind: ExprKind::ScalarVar(key[1..].to_string()), line }
                        } else {
                            Expr { kind: ExprKind::String(key), line }
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
                        if i < chars.len() { i += 1; }
                        let idx_expr = if idx_str.starts_with('$') {
                            Expr { kind: ExprKind::ScalarVar(idx_str[1..].to_string()), line }
                        } else if let Ok(n) = idx_str.parse::<i64>() {
                            Expr { kind: ExprKind::Integer(n), line }
                        } else {
                            Expr { kind: ExprKind::String(idx_str), line }
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
            } else if chars[i] == '@' && i + 1 < chars.len() && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_') {
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
                return Expr { kind: ExprKind::String(s.clone()), line };
            }
        }
        if parts.is_empty() {
            return Expr { kind: ExprKind::String(String::new()), line };
        }

        Expr { kind: ExprKind::InterpolatedString(parts), line }
    }
}
