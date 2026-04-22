//! Zsh lexical analyzer - Direct port from zsh/Src/lex.c
//!
//! This lexer tokenizes zsh shell input into a stream of tokens.
//! It handles all zsh-specific syntax including:
//! - Single/double/dollar quotes
//! - Command substitution $(...)  and `...`
//! - Arithmetic $((...))
//! - Parameter expansion ${...}
//! - Process substitution <(...) >(...)
//! - Here documents
//! - All redirection operators
//! - Comments
//! - Continuation lines

use crate::zsh_tokens::{char_tokens, LexTok};
use std::collections::VecDeque;

/// Lexer flags controlling behavior
#[derive(Debug, Clone, Copy, Default)]
pub struct LexFlags {
    /// Parsing for ZLE (line editor) completion
    pub zle: bool,
    /// Return newlines as tokens
    pub newline: bool,
    /// Preserve comments in output
    pub comments_keep: bool,
    /// Strip comments from output
    pub comments_strip: bool,
    /// Active lexing (from bufferwords)
    pub active: bool,
}

/// Buffer state for building tokens
#[derive(Debug, Clone)]
struct LexBuf {
    data: String,
    siz: usize,
}

impl LexBuf {
    fn new() -> Self {
        LexBuf {
            data: String::with_capacity(256),
            siz: 256,
        }
    }

    fn clear(&mut self) {
        self.data.clear();
    }

    fn add(&mut self, c: char) {
        self.data.push(c);
        if self.data.len() >= self.siz {
            self.siz *= 2;
            self.data.reserve(self.siz - self.data.len());
        }
    }

    fn add_str(&mut self, s: &str) {
        self.data.push_str(s);
    }

    fn len(&self) -> usize {
        self.data.len()
    }

    fn as_str(&self) -> &str {
        &self.data
    }

    fn into_string(self) -> String {
        self.data
    }

    fn last_char(&self) -> Option<char> {
        self.data.chars().last()
    }

    fn pop(&mut self) -> Option<char> {
        self.data.pop()
    }
}

/// Here-document state
#[derive(Debug, Clone)]
pub struct HereDoc {
    pub terminator: String,
    pub strip_tabs: bool,
    pub content: String,
}

/// The Zsh Lexer
pub struct ZshLexer<'a> {
    /// Input source
    input: &'a str,
    /// Current position in input
    pos: usize,
    /// Look-ahead buffer for ungotten characters
    unget_buf: VecDeque<char>,
    /// Current token string
    pub tokstr: Option<String>,
    /// Current token type
    pub tok: LexTok,
    /// File descriptor for redirections (e.g., 2> means fd=2)
    pub tokfd: i32,
    /// Line number at start of current token
    pub toklineno: u64,
    /// Current line number
    pub lineno: u64,
    /// Lexer has stopped (EOF or error)
    pub lexstop: bool,
    /// In command position (can accept reserved words)
    pub incmdpos: bool,
    /// In condition [[ ... ]]
    pub incond: i32,
    /// In case pattern
    pub incasepat: i32,
    /// In redirection
    pub inredir: bool,
    /// After 'for' keyword
    pub infor: i32,
    /// After 'repeat' keyword
    inrepeat: i32,
    /// Parsing typeset arguments
    pub intypeset: bool,
    /// Inside (( ... )) arithmetic
    dbparens: bool,
    /// Disable alias expansion
    pub noaliases: bool,
    /// Disable spelling correction
    pub nocorrect: i32,
    /// Disable comment recognition
    pub nocomments: bool,
    /// Lexer flags
    pub lexflags: LexFlags,
    /// Whether this is the first line
    pub isfirstln: bool,
    /// Whether this is the first char of command
    isfirstch: bool,
    /// Pending here-documents
    pub heredocs: Vec<HereDoc>,
    /// Token buffer
    lexbuf: LexBuf,
    /// After newline
    pub isnewlin: i32,
    /// Error message if any
    pub error: Option<String>,
}

impl<'a> ZshLexer<'a> {
    /// Create a new lexer for the given input
    pub fn new(input: &'a str) -> Self {
        ZshLexer {
            input,
            pos: 0,
            unget_buf: VecDeque::new(),
            tokstr: None,
            tok: LexTok::Endinput,
            tokfd: -1,
            toklineno: 1,
            lineno: 1,
            lexstop: false,
            incmdpos: true,
            incond: 0,
            incasepat: 0,
            inredir: false,
            infor: 0,
            inrepeat: 0,
            intypeset: false,
            dbparens: false,
            noaliases: false,
            nocorrect: 0,
            nocomments: false,
            lexflags: LexFlags::default(),
            isfirstln: true,
            isfirstch: true,
            heredocs: Vec::new(),
            lexbuf: LexBuf::new(),
            isnewlin: 0,
            error: None,
        }
    }

    /// Get next character from input
    fn hgetc(&mut self) -> Option<char> {
        if let Some(c) = self.unget_buf.pop_front() {
            return Some(c);
        }

        let c = self.input[self.pos..].chars().next()?;
        self.pos += c.len_utf8();

        if c == '\n' {
            self.lineno += 1;
        }

        Some(c)
    }

    /// Put character back into input
    fn hungetc(&mut self, c: char) {
        self.unget_buf.push_front(c);
        if c == '\n' && self.lineno > 1 {
            self.lineno -= 1;
        }
        self.lexstop = false;
    }

    /// Peek at next character without consuming
    fn peek(&mut self) -> Option<char> {
        if let Some(&c) = self.unget_buf.front() {
            return Some(c);
        }
        self.input[self.pos..].chars().next()
    }

    /// Add character to token buffer
    fn add(&mut self, c: char) {
        self.lexbuf.add(c);
    }

    /// Check if character is blank (space or tab)
    fn is_blank(c: char) -> bool {
        c == ' ' || c == '\t'
    }

    /// Check if character is blank (including other whitespace except newline)
    fn is_inblank(c: char) -> bool {
        matches!(c, ' ' | '\t' | '\x0b' | '\x0c' | '\r')
    }

    /// Check if character is a digit
    fn is_digit(c: char) -> bool {
        c.is_ascii_digit()
    }

    /// Check if character is identifier start
    fn is_ident_start(c: char) -> bool {
        c.is_ascii_alphabetic() || c == '_'
    }

    /// Check if character is identifier continuation
    fn is_ident(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    /// Main lexer entry point - get next token
    pub fn zshlex(&mut self) {
        if self.tok == LexTok::Lexerr {
            return;
        }

        loop {
            if self.inrepeat > 0 {
                self.inrepeat += 1;
            }
            if self.inrepeat == 3 {
                self.incmdpos = true;
            }

            self.tok = self.gettok();

            // Handle alias expansion would go here
            break;
        }

        self.nocorrect &= 1;

        // Handle here-documents at end of line
        if self.tok == LexTok::Newlin || self.tok == LexTok::Endinput {
            self.process_heredocs();
        }

        if self.tok != LexTok::Newlin {
            self.isnewlin = 0;
        } else {
            self.isnewlin = if self.pos < self.input.len() { -1 } else { 1 };
        }

        if self.tok == LexTok::Semi || (self.tok == LexTok::Newlin && !self.lexflags.newline) {
            self.tok = LexTok::Seper;
        }

        // Check for reserved words when in command position
        // Also check for "{" and "}" which are special in many contexts
        if self.tok == LexTok::String {
            if let Some(ref s) = self.tokstr {
                if s == "{" {
                    self.tok = LexTok::Inbrace;
                } else if s == "}" {
                    self.tok = LexTok::Outbrace;
                } else {
                    self.check_reserved_word();
                }
            }
        }

        // Update command position for next token based on current token
        match self.tok {
            LexTok::Seper
            | LexTok::Newlin
            | LexTok::Semi
            | LexTok::Dsemi
            | LexTok::Semiamp
            | LexTok::Semibar
            | LexTok::Amper
            | LexTok::Amperbang
            | LexTok::Inpar
            | LexTok::Inbrace
            | LexTok::Dbar
            | LexTok::Damper
            | LexTok::Bar
            | LexTok::Baramp
            | LexTok::Inoutpar
            | LexTok::Doloop
            | LexTok::Then
            | LexTok::Elif
            | LexTok::Else
            | LexTok::Doutbrack
            | LexTok::Func => {
                self.incmdpos = true;
            }
            LexTok::String
            | LexTok::Typeset
            | LexTok::Envarray
            | LexTok::Outpar
            | LexTok::Case
            | LexTok::Dinbrack => {
                self.incmdpos = false;
            }
            _ => {}
        }
    }

    /// Process pending here-documents
    fn process_heredocs(&mut self) {
        let heredocs = std::mem::take(&mut self.heredocs);

        for mut hdoc in heredocs {
            let mut content = String::new();

            loop {
                let line = self.read_line();
                if line.is_none() {
                    self.error = Some("here document too large or unterminated".to_string());
                    self.tok = LexTok::Lexerr;
                    return;
                }

                let line = line.unwrap();
                let check_line = if hdoc.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    &line
                };

                if check_line.trim_end_matches('\n') == hdoc.terminator {
                    break;
                }

                content.push_str(&line);
            }

            hdoc.content = content;
        }
    }

    /// Read a line from input
    fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();

        loop {
            let c = self.hgetc()?;
            line.push(c);
            if c == '\n' {
                break;
            }
        }

        Some(line)
    }

    /// Get the next token
    fn gettok(&mut self) -> LexTok {
        self.tokstr = None;
        self.tokfd = -1;

        // Skip whitespace
        loop {
            let c = match self.hgetc() {
                Some(c) => c,
                None => {
                    self.lexstop = true;
                    return if self.error.is_some() {
                        LexTok::Lexerr
                    } else {
                        LexTok::Endinput
                    };
                }
            };

            if !Self::is_blank(c) {
                self.hungetc(c);
                break;
            }
        }

        let c = match self.hgetc() {
            Some(c) => c,
            None => {
                self.lexstop = true;
                return LexTok::Endinput;
            }
        };

        self.toklineno = self.lineno;
        self.isfirstln = false;

        // Handle (( ... )) arithmetic
        if self.dbparens {
            return self.lex_arith(c);
        }

        // Handle digit followed by redirection
        if Self::is_digit(c) {
            let d = self.hgetc();
            match d {
                Some('&') => {
                    let e = self.hgetc();
                    if e == Some('>') {
                        self.tokfd = (c as u8 - b'0') as i32;
                        self.hungetc('>');
                        return self.lex_initial('&');
                    }
                    if let Some(e) = e {
                        self.hungetc(e);
                    }
                    self.hungetc('&');
                }
                Some('>') | Some('<') => {
                    self.tokfd = (c as u8 - b'0') as i32;
                    return self.lex_initial(d.unwrap());
                }
                Some(d) => {
                    self.hungetc(d);
                }
                None => {}
            }
            self.lexstop = false;
        }

        self.lex_initial(c)
    }

    /// Lex (( ... )) arithmetic expression
    fn lex_arith(&mut self, c: char) -> LexTok {
        self.lexbuf.clear();
        self.hungetc(c);

        let end_char = if self.infor > 0 { ';' } else { ')' };
        if self.dquote_parse(end_char, false).is_err() {
            return LexTok::Lexerr;
        }

        self.tokstr = Some(self.lexbuf.as_str().to_string());

        if !self.lexstop && self.infor > 0 {
            self.infor -= 1;
            return LexTok::Dinpar;
        }

        // Check for closing ))
        match self.hgetc() {
            Some(')') => {
                self.dbparens = false;
                LexTok::Doutpar
            }
            c => {
                if let Some(c) = c {
                    self.hungetc(c);
                }
                LexTok::Lexerr
            }
        }
    }

    /// Handle initial character of token
    fn lex_initial(&mut self, c: char) -> LexTok {
        // Handle comments
        if c == '#' && !self.nocomments {
            return self.lex_comment();
        }

        match c {
            '\\' => {
                let d = self.hgetc();
                if d == Some('\n') {
                    // Line continuation - get next token
                    return self.gettok();
                }
                if let Some(d) = d {
                    self.hungetc(d);
                }
                self.lexstop = false;
                self.gettokstr(c, false)
            }

            '\n' => LexTok::Newlin,

            ';' => {
                let d = self.hgetc();
                match d {
                    Some(';') => LexTok::Dsemi,
                    Some('&') => LexTok::Semiamp,
                    Some('|') => LexTok::Semibar,
                    _ => {
                        if let Some(d) = d {
                            self.hungetc(d);
                        }
                        self.lexstop = false;
                        LexTok::Semi
                    }
                }
            }

            '&' => {
                let d = self.hgetc();
                match d {
                    Some('&') => LexTok::Damper,
                    Some('!') | Some('|') => LexTok::Amperbang,
                    Some('>') => {
                        self.tokfd = self.tokfd.max(0);
                        let e = self.hgetc();
                        match e {
                            Some('!') | Some('|') => LexTok::Outangampbang,
                            Some('>') => {
                                let f = self.hgetc();
                                match f {
                                    Some('!') | Some('|') => LexTok::Doutangampbang,
                                    _ => {
                                        if let Some(f) = f {
                                            self.hungetc(f);
                                        }
                                        self.lexstop = false;
                                        LexTok::Doutangamp
                                    }
                                }
                            }
                            _ => {
                                if let Some(e) = e {
                                    self.hungetc(e);
                                }
                                self.lexstop = false;
                                LexTok::Ampoutang
                            }
                        }
                    }
                    _ => {
                        if let Some(d) = d {
                            self.hungetc(d);
                        }
                        self.lexstop = false;
                        LexTok::Amper
                    }
                }
            }

            '|' => {
                let d = self.hgetc();
                match d {
                    Some('|') if self.incasepat <= 0 => LexTok::Dbar,
                    Some('&') => LexTok::Baramp,
                    _ => {
                        if let Some(d) = d {
                            self.hungetc(d);
                        }
                        self.lexstop = false;
                        LexTok::Bar
                    }
                }
            }

            '(' => {
                let d = self.hgetc();
                match d {
                    Some('(') => {
                        if self.infor > 0 {
                            self.dbparens = true;
                            return LexTok::Dinpar;
                        }
                        if self.incmdpos {
                            // Could be (( arithmetic )) or ( subshell )
                            self.lexbuf.clear();
                            match self.cmd_or_math() {
                                CmdOrMath::Math => {
                                    self.tokstr = Some(self.lexbuf.as_str().to_string());
                                    return LexTok::Dinpar;
                                }
                                CmdOrMath::Cmd => {
                                    self.tokstr = None;
                                    return LexTok::Inpar;
                                }
                                CmdOrMath::Err => return LexTok::Lexerr,
                            }
                        }
                        self.hungetc('(');
                        self.lexstop = false;
                        self.gettokstr('(', false)
                    }
                    Some(')') => LexTok::Inoutpar,
                    _ => {
                        if let Some(d) = d {
                            self.hungetc(d);
                        }
                        self.lexstop = false;
                        if self.incond == 1 || self.incmdpos {
                            LexTok::Inpar
                        } else {
                            self.gettokstr('(', false)
                        }
                    }
                }
            }

            ')' => LexTok::Outpar,

            '{' => {
                if self.incmdpos {
                    self.tokstr = Some("{".to_string());
                    LexTok::Inbrace
                } else {
                    self.gettokstr(c, false)
                }
            }

            '}' => {
                if self.incmdpos {
                    self.tokstr = Some("}".to_string());
                    LexTok::Outbrace
                } else {
                    self.gettokstr(c, false)
                }
            }

            '<' => self.lex_inang(),

            '>' => self.lex_outang(),

            _ => self.gettokstr(c, false),
        }
    }

    /// Lex comment
    fn lex_comment(&mut self) -> LexTok {
        if self.lexflags.comments_keep {
            self.lexbuf.clear();
            self.add('#');
        }

        loop {
            let c = self.hgetc();
            match c {
                Some('\n') | None => break,
                Some(c) => {
                    if self.lexflags.comments_keep {
                        self.add(c);
                    }
                }
            }
        }

        if self.lexflags.comments_keep {
            self.tokstr = Some(self.lexbuf.as_str().to_string());
            if !self.lexstop {
                self.hungetc('\n');
            }
            return LexTok::String;
        }

        if self.lexflags.comments_strip && self.lexstop {
            return LexTok::Endinput;
        }

        LexTok::Newlin
    }

    /// Lex < and variants
    fn lex_inang(&mut self) -> LexTok {
        let d = self.hgetc();
        match d {
            Some('(') => {
                // Process substitution <(...)
                self.hungetc('(');
                self.lexstop = false;
                return self.gettokstr('<', false);
            }
            Some('>') => return LexTok::Inoutang,
            Some('<') => {
                let e = self.hgetc();
                match e {
                    Some('(') => {
                        self.hungetc('(');
                        self.hungetc('<');
                        return LexTok::Inang;
                    }
                    Some('<') => return LexTok::Trinang,
                    Some('-') => return LexTok::Dinangdash,
                    _ => {
                        if let Some(e) = e {
                            self.hungetc(e);
                        }
                        self.lexstop = false;
                        return LexTok::Dinang;
                    }
                }
            }
            Some('&') => return LexTok::Inangamp,
            _ => {
                if let Some(d) = d {
                    self.hungetc(d);
                }
                self.lexstop = false;
                return LexTok::Inang;
            }
        }
    }

    /// Lex > and variants
    fn lex_outang(&mut self) -> LexTok {
        let d = self.hgetc();
        match d {
            Some('(') => {
                // Process substitution >(...)
                self.hungetc('(');
                self.lexstop = false;
                return self.gettokstr('>', false);
            }
            Some('&') => {
                let e = self.hgetc();
                match e {
                    Some('!') | Some('|') => return LexTok::Outangampbang,
                    _ => {
                        if let Some(e) = e {
                            self.hungetc(e);
                        }
                        self.lexstop = false;
                        return LexTok::Outangamp;
                    }
                }
            }
            Some('!') | Some('|') => return LexTok::Outangbang,
            Some('>') => {
                let e = self.hgetc();
                match e {
                    Some('&') => {
                        let f = self.hgetc();
                        match f {
                            Some('!') | Some('|') => return LexTok::Doutangampbang,
                            _ => {
                                if let Some(f) = f {
                                    self.hungetc(f);
                                }
                                self.lexstop = false;
                                return LexTok::Doutangamp;
                            }
                        }
                    }
                    Some('!') | Some('|') => return LexTok::Doutangbang,
                    Some('(') => {
                        self.hungetc('(');
                        self.hungetc('>');
                        return LexTok::Outang;
                    }
                    _ => {
                        if let Some(e) = e {
                            self.hungetc(e);
                        }
                        self.lexstop = false;
                        return LexTok::Doutang;
                    }
                }
            }
            _ => {
                if let Some(d) = d {
                    self.hungetc(d);
                }
                self.lexstop = false;
                return LexTok::Outang;
            }
        }
    }

    /// Get rest of token string
    fn gettokstr(&mut self, c: char, sub: bool) -> LexTok {
        let mut bct = 0; // brace count
        let mut pct = 0; // parenthesis count
        let mut brct = 0; // bracket count
        let mut in_brace_param = 0;
        let mut peek = LexTok::String;
        let mut intpos = 1;
        let mut unmatched = '\0';
        let mut c = c;

        if !sub {
            self.lexbuf.clear();
        }

        loop {
            let inbl = Self::is_inblank(c);

            if inbl && in_brace_param == 0 && pct == 0 {
                // Whitespace outside brace param ends token
                break;
            }

            match c {
                // Whitespace is handled above for most cases
                ')' => {
                    if in_brace_param > 0 || sub {
                        self.add(char_tokens::OUTPAR);
                    } else if pct > 0 {
                        pct -= 1;
                        self.add(char_tokens::OUTPAR);
                    } else {
                        break;
                    }
                }

                '|' => {
                    if pct == 0 && in_brace_param == 0 {
                        if sub {
                            self.add(c);
                        } else {
                            break;
                        }
                    } else {
                        self.add(char_tokens::BAR);
                    }
                }

                '$' => {
                    let e = self.hgetc();
                    match e {
                        Some('\\') => {
                            let f = self.hgetc();
                            if f != Some('\n') {
                                if let Some(f) = f {
                                    self.hungetc(f);
                                }
                                self.hungetc('\\');
                                self.add(char_tokens::STRING);
                            } else {
                                // Line continuation after $
                                continue;
                            }
                        }
                        Some('[') => {
                            // $[...] arithmetic
                            self.add(char_tokens::STRING);
                            self.add(char_tokens::INBRACK);
                            if self.dquote_parse(']', sub).is_err() {
                                peek = LexTok::Lexerr;
                                break;
                            }
                            self.add(char_tokens::OUTBRACK);
                        }
                        Some('(') => {
                            // $(...) or $((...))
                            self.add(char_tokens::STRING);
                            match self.cmd_or_math_sub() {
                                CmdOrMath::Cmd => self.add(char_tokens::OUTPAR),
                                CmdOrMath::Math => self.add(char_tokens::OUTPARMATH),
                                CmdOrMath::Err => {
                                    peek = LexTok::Lexerr;
                                    break;
                                }
                            }
                        }
                        Some('{') => {
                            self.add(c);
                            self.add(char_tokens::INBRACE);
                            bct += 1;
                            if in_brace_param == 0 {
                                in_brace_param = bct;
                            }
                        }
                        _ => {
                            if let Some(e) = e {
                                self.hungetc(e);
                            }
                            self.lexstop = false;
                            self.add(char_tokens::STRING);
                        }
                    }
                }

                '[' => {
                    if in_brace_param == 0 {
                        brct += 1;
                    }
                    self.add(char_tokens::INBRACK);
                }

                ']' => {
                    if in_brace_param == 0 && brct > 0 {
                        brct -= 1;
                    }
                    self.add(char_tokens::OUTBRACK);
                }

                '(' => {
                    if in_brace_param == 0 {
                        pct += 1;
                    }
                    self.add(char_tokens::INPAR);
                }

                '{' => {
                    if in_brace_param > 0 {
                        bct += 1;
                    }
                    self.add(c);
                }

                '}' => {
                    if in_brace_param > 0 {
                        if bct == in_brace_param {
                            in_brace_param = 0;
                        }
                        bct -= 1;
                        self.add(char_tokens::OUTBRACE);
                    } else if bct > 0 {
                        bct -= 1;
                        self.add(char_tokens::OUTBRACE);
                    } else {
                        break;
                    }
                }

                '>' => {
                    if in_brace_param > 0 || sub {
                        self.add(c);
                    } else {
                        let e = self.hgetc();
                        if e != Some('(') {
                            if let Some(e) = e {
                                self.hungetc(e);
                            }
                            self.lexstop = false;
                            break;
                        }
                        // >(...)
                        self.add(char_tokens::OUTANGPROC);
                        if self.skip_command_sub().is_err() {
                            peek = LexTok::Lexerr;
                            break;
                        }
                        self.add(char_tokens::OUTPAR);
                    }
                }

                '<' => {
                    if in_brace_param > 0 || sub {
                        self.add(c);
                    } else {
                        let e = self.hgetc();
                        if e != Some('(') {
                            if let Some(e) = e {
                                self.hungetc(e);
                            }
                            self.lexstop = false;
                            break;
                        }
                        // <(...)
                        self.add(char_tokens::INANG);
                        if self.skip_command_sub().is_err() {
                            peek = LexTok::Lexerr;
                            break;
                        }
                        self.add(char_tokens::OUTPAR);
                    }
                }

                '=' => {
                    if !sub {
                        if intpos > 0 {
                            // At start of token, check for =(...) process substitution
                            let e = self.hgetc();
                            if e == Some('(') {
                                self.add(char_tokens::EQUALS);
                                if self.skip_command_sub().is_err() {
                                    peek = LexTok::Lexerr;
                                    break;
                                }
                                self.add(char_tokens::OUTPAR);
                            } else {
                                if let Some(e) = e {
                                    self.hungetc(e);
                                }
                                self.lexstop = false;
                                self.add(char_tokens::EQUALS);
                            }
                        } else if peek != LexTok::Envstring
                            && (self.incmdpos || self.intypeset)
                            && bct == 0
                            && brct == 0
                        {
                            // Check for VAR=value assignment
                            let tok_so_far = self.lexbuf.as_str().to_string();
                            if self.is_valid_assignment_target(&tok_so_far) {
                                let next = self.hgetc();
                                if next == Some('(') {
                                    // VAR=(...) array assignment
                                    self.tokstr = Some(tok_so_far);
                                    return LexTok::Envarray;
                                }
                                if let Some(next) = next {
                                    self.hungetc(next);
                                }
                                self.lexstop = false;
                                peek = LexTok::Envstring;
                                intpos = 2;
                                self.add(char_tokens::EQUALS);
                            } else {
                                self.add(char_tokens::EQUALS);
                            }
                        } else {
                            self.add(char_tokens::EQUALS);
                        }
                    } else {
                        self.add(char_tokens::EQUALS);
                    }
                }

                '\\' => {
                    let next = self.hgetc();
                    if next == Some('\n') {
                        // Line continuation
                        let next = self.hgetc();
                        if let Some(next) = next {
                            c = next;
                            continue;
                        }
                        break;
                    } else {
                        self.add(char_tokens::BNULL);
                        if let Some(next) = next {
                            self.add(next);
                        }
                    }
                }

                '\'' => {
                    // Single quoted string - everything literal until '
                    self.add(char_tokens::SNULL);
                    loop {
                        let ch = self.hgetc();
                        match ch {
                            Some('\'') => break,
                            Some(ch) => self.add(ch),
                            None => {
                                self.lexstop = true;
                                unmatched = '\'';
                                peek = LexTok::Lexerr;
                                break;
                            }
                        }
                    }
                    if unmatched != '\0' {
                        break;
                    }
                    self.add(char_tokens::SNULL);
                }

                '"' => {
                    // Double quoted string
                    self.add(char_tokens::DNULL);
                    if self.dquote_parse('"', sub).is_err() {
                        unmatched = '"';
                        if !self.lexflags.active {
                            peek = LexTok::Lexerr;
                        }
                        break;
                    }
                    self.add(char_tokens::DNULL);
                }

                '`' => {
                    // Backtick command substitution
                    self.add(char_tokens::TICK);
                    loop {
                        let ch = self.hgetc();
                        match ch {
                            Some('`') => break,
                            Some('\\') => {
                                let next = self.hgetc();
                                match next {
                                    Some('\n') => continue, // Line continuation
                                    Some(c) if c == '`' || c == '\\' || c == '$' => {
                                        self.add(char_tokens::BNULL);
                                        self.add(c);
                                    }
                                    Some(c) => {
                                        self.add('\\');
                                        self.add(c);
                                    }
                                    None => break,
                                }
                            }
                            Some(ch) => self.add(ch),
                            None => {
                                self.lexstop = true;
                                unmatched = '`';
                                peek = LexTok::Lexerr;
                                break;
                            }
                        }
                    }
                    if unmatched != '\0' {
                        break;
                    }
                    self.add(char_tokens::TICK);
                }

                '~' => {
                    self.add(char_tokens::TILDE);
                }

                '#' => {
                    self.add(char_tokens::POUND);
                }

                '^' => {
                    self.add(char_tokens::HAT);
                }

                '*' => {
                    self.add(char_tokens::STAR);
                }

                '?' => {
                    self.add(char_tokens::QUEST);
                }

                ',' => {
                    if bct > in_brace_param {
                        self.add(char_tokens::COMMA);
                    } else {
                        self.add(c);
                    }
                }

                '-' => {
                    self.add(char_tokens::DASH);
                }

                '!' => {
                    if brct > 0 {
                        self.add(char_tokens::BANG);
                    } else {
                        self.add(c);
                    }
                }

                // Terminators
                '\n' | ';' | '&' => {
                    break;
                }

                _ => {
                    self.add(c);
                }
            }

            c = match self.hgetc() {
                Some(c) => c,
                None => {
                    self.lexstop = true;
                    break;
                }
            };

            if intpos > 0 {
                intpos -= 1;
            }
        }

        // Put back the character that ended the token
        if !self.lexstop {
            self.hungetc(c);
        }

        if unmatched != '\0' && !self.lexflags.active {
            self.error = Some(format!("unmatched {}", unmatched));
        }

        if in_brace_param > 0 {
            self.error = Some("closing brace expected".to_string());
        }

        self.tokstr = Some(self.lexbuf.as_str().to_string());
        peek
    }

    /// Check if a string is a valid assignment target (identifier or array ref)
    fn is_valid_assignment_target(&self, s: &str) -> bool {
        let mut chars = s.chars().peekable();

        // Check for leading digit (invalid)
        if let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                // Could be array index, check rest
                while let Some(&c) = chars.peek() {
                    if !c.is_ascii_digit() {
                        break;
                    }
                    chars.next();
                }
                return chars.peek().is_none();
            }
        }

        // Check identifier
        let mut has_ident = false;
        while let Some(&c) = chars.peek() {
            if c == char_tokens::INBRACK || c == '[' {
                break;
            }
            if c == '+' {
                // foo+=value
                chars.next();
                return chars.peek().is_none() || chars.peek() == Some(&'=');
            }
            if !Self::is_ident(c) && c != char_tokens::STRING && !char_tokens::is_token(c) {
                return false;
            }
            has_ident = true;
            chars.next();
        }

        has_ident
    }

    /// Parse double-quoted string content
    fn dquote_parse(&mut self, endchar: char, sub: bool) -> Result<(), ()> {
        let mut pct = 0; // parenthesis count
        let mut brct = 0; // bracket count
        let mut bct = 0; // brace count (for ${...})
        let mut intick = false; // inside backtick
        let is_math = endchar == ')' || endchar == ']' || self.infor > 0;

        loop {
            let c = self.hgetc();
            let c = match c {
                Some(c) if c == endchar && !intick && bct == 0 => {
                    if is_math && (pct > 0 || brct > 0) {
                        self.add(c);
                        if c == ')' {
                            pct -= 1;
                        } else if c == ']' {
                            brct -= 1;
                        }
                        continue;
                    }
                    return Ok(());
                }
                Some(c) => c,
                None => {
                    self.lexstop = true;
                    return Err(());
                }
            };

            match c {
                '\\' => {
                    let next = self.hgetc();
                    match next {
                        Some('\n') if !sub => continue, // Line continuation
                        Some(c)
                            if c == '$'
                                || c == '\\'
                                || (c == '}' && !intick && bct > 0)
                                || c == endchar
                                || c == '`'
                                || (endchar == ']'
                                    && (c == '['
                                        || c == ']'
                                        || c == '('
                                        || c == ')'
                                        || c == '{'
                                        || c == '}'
                                        || (c == '"' && sub))) =>
                        {
                            self.add(char_tokens::BNULL);
                            self.add(c);
                        }
                        Some(c) => {
                            self.add('\\');
                            self.hungetc(c);
                            continue;
                        }
                        None => {
                            self.add('\\');
                        }
                    }
                }

                '$' => {
                    if intick {
                        self.add(c);
                        continue;
                    }
                    let next = self.hgetc();
                    match next {
                        Some('(') => {
                            self.add(char_tokens::QSTRING);
                            match self.cmd_or_math_sub() {
                                CmdOrMath::Cmd => self.add(char_tokens::OUTPAR),
                                CmdOrMath::Math => self.add(char_tokens::OUTPARMATH),
                                CmdOrMath::Err => return Err(()),
                            }
                        }
                        Some('[') => {
                            self.add(char_tokens::STRING);
                            self.add(char_tokens::INBRACK);
                            self.dquote_parse(']', sub)?;
                            self.add(char_tokens::OUTBRACK);
                        }
                        Some('{') => {
                            self.add(char_tokens::QSTRING);
                            self.add(char_tokens::INBRACE);
                            bct += 1;
                        }
                        Some('$') => {
                            self.add(char_tokens::QSTRING);
                            self.add('$');
                        }
                        _ => {
                            if let Some(next) = next {
                                self.hungetc(next);
                            }
                            self.lexstop = false;
                            self.add(char_tokens::QSTRING);
                        }
                    }
                }

                '}' => {
                    if intick || bct == 0 {
                        self.add(c);
                    } else {
                        self.add(char_tokens::OUTBRACE);
                        bct -= 1;
                    }
                }

                '`' => {
                    self.add(char_tokens::QTICK);
                    intick = !intick;
                }

                '(' => {
                    if !is_math || bct == 0 {
                        pct += 1;
                    }
                    self.add(c);
                }

                ')' => {
                    if !is_math || bct == 0 {
                        if pct == 0 && is_math {
                            return Err(());
                        }
                        pct -= 1;
                    }
                    self.add(c);
                }

                '[' => {
                    if !is_math || bct == 0 {
                        brct += 1;
                    }
                    self.add(c);
                }

                ']' => {
                    if !is_math || bct == 0 {
                        if brct == 0 && is_math {
                            return Err(());
                        }
                        brct -= 1;
                    }
                    self.add(c);
                }

                '"' => {
                    if intick || (endchar != '"' && bct == 0) {
                        self.add(c);
                    } else if bct > 0 {
                        self.add(char_tokens::DNULL);
                        self.dquote_parse('"', sub)?;
                        self.add(char_tokens::DNULL);
                    } else {
                        return Err(());
                    }
                }

                _ => {
                    self.add(c);
                }
            }
        }
    }

    /// Determine if (( is arithmetic or command
    fn cmd_or_math(&mut self) -> CmdOrMath {
        let oldlen = self.lexbuf.len();

        self.add(char_tokens::INPAR);
        self.add('(');

        if self.dquote_parse(')', false).is_err() {
            // Back up and try as command
            while self.lexbuf.len() > oldlen {
                if let Some(c) = self.lexbuf.pop() {
                    self.hungetc(c);
                }
            }
            self.hungetc('(');
            self.lexstop = false;
            return if self.skip_command_sub().is_err() {
                CmdOrMath::Err
            } else {
                CmdOrMath::Cmd
            };
        }

        // Check for closing )
        let c = self.hgetc();
        if c == Some(')') {
            self.add(')');
            return CmdOrMath::Math;
        }

        // Not math, back up
        if let Some(c) = c {
            self.hungetc(c);
        }
        self.lexstop = false;

        // Back up token
        while self.lexbuf.len() > oldlen {
            if let Some(c) = self.lexbuf.pop() {
                self.hungetc(c);
            }
        }
        self.hungetc('(');

        if self.skip_command_sub().is_err() {
            CmdOrMath::Err
        } else {
            CmdOrMath::Cmd
        }
    }

    /// Parse $(...) or $((...))
    fn cmd_or_math_sub(&mut self) -> CmdOrMath {
        let c = self.hgetc();
        if c == Some('\\') {
            let c = self.hgetc();
            if c != Some('\n') {
                if let Some(c) = c {
                    self.hungetc(c);
                }
                self.hungetc('\\');
                self.lexstop = false;
                return if self.skip_command_sub().is_err() {
                    CmdOrMath::Err
                } else {
                    CmdOrMath::Cmd
                };
            }
            // Line continuation, try again
            return self.cmd_or_math_sub();
        }

        if c == Some('(') {
            // Might be $((...))
            let lexpos = self.lexbuf.len();
            self.add(char_tokens::INPAR);
            self.add('(');

            if self.dquote_parse(')', false).is_ok() {
                let c = self.hgetc();
                if c == Some(')') {
                    self.add(')');
                    return CmdOrMath::Math;
                }
                if let Some(c) = c {
                    self.hungetc(c);
                }
            }

            // Not math, restore and parse as command
            while self.lexbuf.len() > lexpos {
                if let Some(c) = self.lexbuf.pop() {
                    self.hungetc(c);
                }
            }
            self.hungetc('(');
            self.lexstop = false;
        } else {
            if let Some(c) = c {
                self.hungetc(c);
            }
            self.lexstop = false;
        }

        if self.skip_command_sub().is_err() {
            CmdOrMath::Err
        } else {
            CmdOrMath::Cmd
        }
    }

    /// Skip over command substitution (...), adding chars to token
    fn skip_command_sub(&mut self) -> Result<(), ()> {
        let mut pct = 1;
        let mut start = true;

        self.add(char_tokens::INPAR);

        loop {
            let c = self.hgetc();
            let c = match c {
                Some(c) => c,
                None => {
                    self.lexstop = true;
                    return Err(());
                }
            };

            let iswhite = Self::is_inblank(c);

            match c {
                '(' => {
                    pct += 1;
                    self.add(c);
                }
                ')' => {
                    pct -= 1;
                    if pct == 0 {
                        return Ok(());
                    }
                    self.add(c);
                }
                '\\' => {
                    self.add(c);
                    if let Some(c) = self.hgetc() {
                        self.add(c);
                    }
                }
                '\'' => {
                    self.add(c);
                    loop {
                        let ch = self.hgetc();
                        match ch {
                            Some('\'') => {
                                self.add('\'');
                                break;
                            }
                            Some(ch) => self.add(ch),
                            None => {
                                self.lexstop = true;
                                return Err(());
                            }
                        }
                    }
                }
                '"' => {
                    self.add(c);
                    loop {
                        let ch = self.hgetc();
                        match ch {
                            Some('"') => {
                                self.add('"');
                                break;
                            }
                            Some('\\') => {
                                self.add('\\');
                                if let Some(ch) = self.hgetc() {
                                    self.add(ch);
                                }
                            }
                            Some(ch) => self.add(ch),
                            None => {
                                self.lexstop = true;
                                return Err(());
                            }
                        }
                    }
                }
                '`' => {
                    self.add(c);
                    loop {
                        let ch = self.hgetc();
                        match ch {
                            Some('`') => {
                                self.add('`');
                                break;
                            }
                            Some('\\') => {
                                self.add('\\');
                                if let Some(ch) = self.hgetc() {
                                    self.add(ch);
                                }
                            }
                            Some(ch) => self.add(ch),
                            None => {
                                self.lexstop = true;
                                return Err(());
                            }
                        }
                    }
                }
                '#' => {
                    if start {
                        self.add(c);
                        // Skip comment to end of line
                        loop {
                            let ch = self.hgetc();
                            match ch {
                                Some('\n') => {
                                    self.add('\n');
                                    break;
                                }
                                Some(ch) => self.add(ch),
                                None => break,
                            }
                        }
                    } else {
                        self.add(c);
                    }
                }
                _ => {
                    self.add(c);
                }
            }

            start = iswhite;
        }
    }

    /// Update parser state after lexing based on token type
    pub fn ctxtlex(&mut self) {
        self.zshlex();

        match self.tok {
            LexTok::Seper
            | LexTok::Newlin
            | LexTok::Semi
            | LexTok::Dsemi
            | LexTok::Semiamp
            | LexTok::Semibar
            | LexTok::Amper
            | LexTok::Amperbang
            | LexTok::Inpar
            | LexTok::Inbrace
            | LexTok::Dbar
            | LexTok::Damper
            | LexTok::Bar
            | LexTok::Baramp
            | LexTok::Inoutpar
            | LexTok::Doloop
            | LexTok::Then
            | LexTok::Elif
            | LexTok::Else
            | LexTok::Doutbrack => {
                self.incmdpos = true;
            }

            LexTok::String
            | LexTok::Typeset
            | LexTok::Envarray
            | LexTok::Outpar
            | LexTok::Case
            | LexTok::Dinbrack => {
                self.incmdpos = false;
            }

            _ => {}
        }

        if self.tok != LexTok::Dinpar {
            self.infor = if self.tok == LexTok::For { 2 } else { 0 };
        }

        let oldpos = self.incmdpos;
        if self.tok.is_redirop()
            || self.tok == LexTok::For
            || self.tok == LexTok::Foreach
            || self.tok == LexTok::Select
        {
            self.inredir = true;
            self.incmdpos = false;
        } else if self.inredir {
            self.incmdpos = oldpos;
            self.inredir = false;
        }
    }

    /// Check for reserved word
    pub fn check_reserved_word(&mut self) -> bool {
        if let Some(ref tokstr) = self.tokstr {
            if self.incmdpos || (tokstr == "}" && self.tok == LexTok::String) {
                if let Some(tok) = crate::zsh_tokens::lookup_reserved_word(tokstr) {
                    self.tok = tok;
                    if tok == LexTok::Repeat {
                        self.inrepeat = 1;
                    }
                    if tok == LexTok::Dinbrack {
                        self.incond = 1;
                    }
                    return true;
                }
                if tokstr == "]]" && self.incond > 0 {
                    self.tok = LexTok::Doutbrack;
                    self.incond = 0;
                    return true;
                }
            }
        }
        false
    }
}

/// Result of determining if (( is arithmetic or command
enum CmdOrMath {
    Cmd,
    Math,
    Err,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let mut lexer = ZshLexer::new("echo hello");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
        assert_eq!(lexer.tokstr, Some("echo".to_string()));

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
        assert_eq!(lexer.tokstr, Some("hello".to_string()));

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::Endinput);
    }

    #[test]
    fn test_pipeline() {
        let mut lexer = ZshLexer::new("ls | grep foo");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::Bar);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
    }

    #[test]
    fn test_redirections() {
        let mut lexer = ZshLexer::new("echo > file");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::Outang);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
    }

    #[test]
    fn test_heredoc() {
        let mut lexer = ZshLexer::new("cat << EOF");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::Dinang);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
    }

    #[test]
    fn test_single_quotes() {
        let mut lexer = ZshLexer::new("echo 'hello world'");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
        // Should contain Snull markers around literal content
        assert!(lexer.tokstr.is_some());
    }

    #[test]
    fn test_function_tokens() {
        let mut lexer = ZshLexer::new("function foo { }");
        lexer.zshlex();
        assert_eq!(
            lexer.tok,
            LexTok::Func,
            "expected Func, got {:?}",
            lexer.tok
        );

        lexer.zshlex();
        assert_eq!(
            lexer.tok,
            LexTok::String,
            "expected String for 'foo', got {:?}",
            lexer.tok
        );
        assert_eq!(lexer.tokstr, Some("foo".to_string()));

        lexer.zshlex();
        assert_eq!(
            lexer.tok,
            LexTok::Inbrace,
            "expected Inbrace, got {:?} tokstr={:?}",
            lexer.tok,
            lexer.tokstr
        );

        lexer.zshlex();
        assert_eq!(
            lexer.tok,
            LexTok::Outbrace,
            "expected Outbrace, got {:?} tokstr={:?} incmdpos={}",
            lexer.tok,
            lexer.tokstr,
            lexer.incmdpos
        );
    }

    #[test]
    fn test_double_quotes() {
        let mut lexer = ZshLexer::new("echo \"hello $name\"");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
        // Should contain tokenized content
        assert!(lexer.tokstr.is_some());
    }

    #[test]
    fn test_command_substitution() {
        let mut lexer = ZshLexer::new("echo $(pwd)");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
    }

    #[test]
    fn test_env_assignment() {
        let mut lexer = ZshLexer::new("FOO=bar echo");
        lexer.incmdpos = true;
        lexer.zshlex();
        assert_eq!(
            lexer.tok,
            LexTok::Envstring,
            "tok={:?} tokstr={:?}",
            lexer.tok,
            lexer.tokstr
        );

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
    }

    #[test]
    fn test_array_assignment() {
        let mut lexer = ZshLexer::new("arr=(a b c)");
        lexer.incmdpos = true;
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::Envarray);
    }

    #[test]
    fn test_process_substitution() {
        let mut lexer = ZshLexer::new("diff <(ls) >(cat)");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
        // <(ls) is tokenized into the string

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
        // >(cat) is tokenized
    }

    #[test]
    fn test_arithmetic() {
        let mut lexer = ZshLexer::new("echo $((1+2))");
        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);

        lexer.zshlex();
        assert_eq!(lexer.tok, LexTok::String);
    }

    #[test]
    fn test_semicolon_variants() {
        let mut lexer = ZshLexer::new("case x in a) cmd;; b) cmd;& c) cmd;| esac");

        // Skip to first ;;
        loop {
            lexer.zshlex();
            if lexer.tok == LexTok::Dsemi || lexer.tok == LexTok::Endinput {
                break;
            }
        }
        assert_eq!(lexer.tok, LexTok::Dsemi);

        // Find ;&
        loop {
            lexer.zshlex();
            if lexer.tok == LexTok::Semiamp || lexer.tok == LexTok::Endinput {
                break;
            }
        }
        assert_eq!(lexer.tok, LexTok::Semiamp);

        // Find ;|
        loop {
            lexer.zshlex();
            if lexer.tok == LexTok::Semibar || lexer.tok == LexTok::Endinput {
                break;
            }
        }
        assert_eq!(lexer.tok, LexTok::Semibar);
    }
}
