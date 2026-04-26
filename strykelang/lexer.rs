use crate::error::{ErrorKind, PerlError, PerlResult};
use crate::token::{keyword_or_ident, Token};

/// Private-use character for a literal `$` inside double-quoted / `qq` strings (from `\$` in source).
/// The parser maps this to `$` without variable interpolation (CPAN `eval qq/…/` code generators).
pub const LITERAL_DOLLAR_IN_DQUOTE: char = '\u{E000}';

/// Resolve `\N{U+XXXX}` hex codepoints and `\N{LATIN SMALL LETTER E}` Unicode character names.
fn parse_unicode_name(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("U+") {
        let val = u32::from_str_radix(hex, 16).ok()?;
        char::from_u32(val)
    } else {
        unicode_names2::character(name)
    }
}

/// Flag letters after `m//`, `qr//`, etc. (`c` = `/gc`, `o` = compile once; CPAN uses both).
const REGEX_FLAG_CHARS: &str = "gimsxecor";

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    pub line: usize,
    /// Tracks whether the last token was a term (value/variable/close-delim)
    /// to disambiguate `/` as division vs regex and `{` as hash-ref vs block.
    last_was_term: bool,
    /// Source path for [`PerlError`] (e.g. real script or required `.pm` path).
    error_file: String,
    /// When > 0, the lexer treats `m` followed by `/` as a plain identifier
    /// instead of `m//` regex syntax. Used in thread/pipeline stages where
    /// `/m/` should be a regex grep filter, not `m//`.
    pub suppress_m_regex: u32,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self::new_with_file(input, "-e")
    }

    pub fn new_with_file(input: &str, file: impl Into<String>) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            last_was_term: false,
            error_file: file.into(),
            suppress_m_regex: 0,
        }
    }

    fn syntax_err(&self, message: impl Into<String>, line: usize) -> PerlError {
        PerlError::new(ErrorKind::Syntax, message, line, self.error_file.clone())
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.input.get(self.pos + offset).copied()
    }

    /// True when `=` at `eq_pos` is Perl POD (`=head1`, `=cut`, …): first non-whitespace on the line.
    /// Otherwise `$_=foo` would misparse `=f` as POD and swallow the rest of the file.
    fn at_line_start_for_pod(&self, eq_pos: usize) -> bool {
        let mut i = eq_pos;
        while i > 0 {
            i -= 1;
            let c = self.input[i];
            if c == '\n' {
                return true;
            }
            if !c.is_whitespace() {
                return false;
            }
        }
        true
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.input.get(self.pos).copied();
        if let Some(c) = ch {
            if c == '\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch == '#' {
                // Line comment
                while self.pos < self.input.len() && self.input[self.pos] != '\n' {
                    self.pos += 1;
                }
            } else if ch == '\\' && self.peek_at(1) == Some('\n') {
                // Backslash-newline: line continuation (shell-style)
                // Don't increment line — continued line is logically part of the same line
                self.pos += 2;
            } else if ch.is_whitespace() {
                if ch == '\n' {
                    self.line += 1;
                }
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// Whitespace only — used after `q`/`qq`/`qr`/… before the opening delimiter so `#` is not
    /// mistaken for a line comment (`qr#...#`, `qw#...#`).
    fn skip_whitespace_only(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input[self.pos];
            if ch.is_whitespace() {
                if ch == '\n' {
                    self.line += 1;
                }
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn read_while(&mut self, pred: impl Fn(char) -> bool) -> String {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if pred(ch) {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_number(&mut self) -> PerlResult<Token> {
        let start = self.pos;
        let mut is_float = false;
        let mut is_hex = false;
        let mut is_oct = false;
        let mut is_bin = false;

        if self.peek() == Some('0') {
            match self.peek_at(1) {
                Some('x') | Some('X') => {
                    is_hex = true;
                    self.advance();
                    self.advance();
                }
                Some('b') | Some('B') => {
                    is_bin = true;
                    self.advance();
                    self.advance();
                }
                Some(c) if c.is_ascii_digit() => {
                    is_oct = true;
                }
                _ => {}
            }
        }

        if is_hex {
            let digits = self.read_while(|c| c.is_ascii_hexdigit() || c == '_');
            let clean: String = digits.chars().filter(|&c| c != '_').collect();
            let val = i64::from_str_radix(&clean, 16)
                .map_err(|_| self.syntax_err("Invalid hex literal", self.line))?;
            return Ok(Token::Integer(val));
        }
        if is_bin {
            let digits = self.read_while(|c| c == '0' || c == '1' || c == '_');
            let clean: String = digits.chars().filter(|&c| c != '_').collect();
            let val = i64::from_str_radix(&clean, 2)
                .map_err(|_| self.syntax_err("Invalid binary literal", self.line))?;
            return Ok(Token::Integer(val));
        }

        // Decimal or octal
        let _int_part = self.read_while(|c| c.is_ascii_digit() || c == '_');
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.advance(); // consume '.'
            let _frac = self.read_while(|c| c.is_ascii_digit() || c == '_');
        }
        // Scientific notation
        if let Some('e') | Some('E') = self.peek() {
            is_float = true;
            self.advance();
            if let Some('+') | Some('-') = self.peek() {
                self.advance();
            }
            let _exp = self.read_while(|c| c.is_ascii_digit() || c == '_');
        }

        let raw: String = self.input[start..self.pos].iter().collect();
        let clean: String = raw.chars().filter(|&c| c != '_').collect();

        if is_float {
            let val: f64 = clean
                .parse()
                .map_err(|_| self.syntax_err("Invalid float literal", self.line))?;
            Ok(Token::Float(val))
        } else if is_oct && clean.starts_with('0') && clean.len() > 1 {
            let val = i64::from_str_radix(&clean[1..], 8)
                .map_err(|_| self.syntax_err("Invalid octal literal", self.line))?;
            Ok(Token::Integer(val))
        } else {
            let val: i64 = clean
                .parse()
                .map_err(|_| self.syntax_err("Invalid integer literal", self.line))?;
            Ok(Token::Integer(val))
        }
    }

    fn read_single_quoted_string(&mut self) -> PerlResult<Token> {
        self.advance(); // consume opening '
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\\') => match self.peek() {
                    Some('\\') => {
                        s.push('\\');
                        self.advance();
                    }
                    Some('\'') => {
                        s.push('\'');
                        self.advance();
                    }
                    _ => s.push('\\'),
                },
                Some('\'') => break,
                Some(c) => s.push(c),
                None => return Err(self.syntax_err("Unterminated single-quoted string", self.line)),
            }
        }
        Ok(Token::SingleString(s))
    }

    fn read_double_quoted_string(&mut self) -> PerlResult<Token> {
        self.advance(); // consume opening "
        let s = self.read_escaped_until('"')?;
        Ok(Token::DoubleString(s))
    }

    fn read_escaped_until(&mut self, term: char) -> PerlResult<String> {
        let mut s = String::new();
        loop {
            match self.advance() {
                Some('\\') => match self.advance() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some(c @ '0'..='7') => {
                        let mut oct = String::new();
                        oct.push(c);
                        for _ in 0..2 {
                            match self.peek() {
                                Some(d) if ('0'..='7').contains(&d) => {
                                    oct.push(self.advance().unwrap());
                                }
                                _ => break,
                            }
                        }
                        let val = u32::from_str_radix(&oct, 8).unwrap();
                        let ch = char::from_u32(val)
                            .ok_or_else(|| self.syntax_err("Invalid octal escape", self.line))?;
                        s.push(ch);
                    }
                    Some('a') => s.push('\x07'),
                    Some('b') => s.push('\x08'),
                    Some('f') => s.push('\x0C'),
                    Some('e') => s.push('\x1B'),
                    Some('$') => s.push(LITERAL_DOLLAR_IN_DQUOTE),
                    Some('c') => {
                        let ch = self
                            .advance()
                            .ok_or_else(|| self.syntax_err("Unterminated \\c escape", self.line))?;
                        s.push(char::from(ch.to_ascii_uppercase() as u8 ^ 0x40));
                    }
                    Some('o') if self.peek() == Some('{') => {
                        self.advance(); // '{'
                        let oct = self.read_while(|c| c != '}');
                        if self.peek() != Some('}') {
                            return Err(
                                self.syntax_err("Unterminated \\o{...} in string", self.line)
                            );
                        }
                        self.advance(); // '}'
                        if oct.is_empty() {
                            return Err(self.syntax_err("Empty \\o{} in string", self.line));
                        }
                        let val = u32::from_str_radix(&oct, 8).map_err(|_| {
                            self.syntax_err("Invalid octal digits in \\o{...}", self.line)
                        })?;
                        let c = char::from_u32(val).ok_or_else(|| {
                            self.syntax_err("Invalid Unicode scalar value in \\o{...}", self.line)
                        })?;
                        s.push(c);
                    }
                    Some('u') if self.peek() == Some('{') => {
                        self.advance(); // '{'
                        let hex = self.read_while(|c| c != '}');
                        if self.peek() != Some('}') {
                            return Err(
                                self.syntax_err("Unterminated \\u{...} in string", self.line)
                            );
                        }
                        self.advance(); // '}'
                        if hex.is_empty() {
                            return Err(self.syntax_err("Empty \\u{} in string", self.line));
                        }
                        let val = u32::from_str_radix(&hex, 16).map_err(|_| {
                            self.syntax_err("Invalid hex digits in \\u{...}", self.line)
                        })?;
                        let c = char::from_u32(val).ok_or_else(|| {
                            self.syntax_err("Invalid Unicode scalar value in \\u{...}", self.line)
                        })?;
                        s.push(c);
                    }
                    Some('N') if self.peek() == Some('{') => {
                        self.advance(); // '{'
                        let name = self.read_while(|c| c != '}');
                        if self.peek() != Some('}') {
                            return Err(
                                self.syntax_err("Unterminated \\N{...} in string", self.line)
                            );
                        }
                        self.advance(); // '}'
                        if name.is_empty() {
                            return Err(self.syntax_err("Empty \\N{} in string", self.line));
                        }
                        let c = parse_unicode_name(&name).ok_or_else(|| {
                            self.syntax_err(
                                format!("Unknown Unicode character name: {name}"),
                                self.line,
                            )
                        })?;
                        s.push(c);
                    }
                    Some('x') => {
                        if self.peek() == Some('{') {
                            self.advance(); // '{'
                            let hex = self.read_while(|c| c != '}');
                            if self.peek() != Some('}') {
                                return Err(
                                    self.syntax_err("Unterminated \\x{...} in string", self.line)
                                );
                            }
                            self.advance(); // '}'
                            if hex.is_empty() {
                                return Err(self.syntax_err("Empty \\x{} in string", self.line));
                            }
                            let val = u32::from_str_radix(&hex, 16).map_err(|_| {
                                self.syntax_err("Invalid hex digits in \\x{...}", self.line)
                            })?;
                            let c = char::from_u32(val).ok_or_else(|| {
                                self.syntax_err(
                                    "Invalid Unicode scalar value in \\x{...}",
                                    self.line,
                                )
                            })?;
                            s.push(c);
                        } else {
                            // Unbraced: up to two hex digits (Perl: "\\x414" is "\\x41" + "4").
                            let mut hex = String::new();
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(c) if c.is_ascii_hexdigit() => {
                                        hex.push(self.advance().unwrap());
                                    }
                                    _ => break,
                                }
                            }
                            if hex.is_empty() {
                                // Perl: bare "\\x" in a string yields NUL.
                                s.push('\0');
                            } else if let Ok(val) = u32::from_str_radix(&hex, 16) {
                                if let Some(c) = char::from_u32(val) {
                                    s.push(c);
                                } else {
                                    return Err(self.syntax_err(
                                        "Invalid code point in \\x escape",
                                        self.line,
                                    ));
                                }
                            }
                        }
                    }
                    Some(c) if c == term => s.push(c),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => return Err(self.syntax_err("Unterminated string", self.line)),
                },
                Some(c) if c == term => break,
                Some(c) => s.push(c),
                None => return Err(self.syntax_err("Unterminated string", self.line)),
            }
        }
        Ok(s)
    }

    /// `q(...)` / `qq(...)` with pairing delimiters — Perl balances nested `()`, `[]`, `{}`, `<>`
    /// so `q(sub ($) { 1 })` does not end at the `)` in `($)` (core `Carp.pm` uses `eval(q(...))`).
    fn read_q_qq_balanced_body(
        &mut self,
        open: char,
        close: char,
        is_qq: bool,
    ) -> PerlResult<String> {
        let mut s = String::new();
        let mut depth: usize = 1;
        loop {
            match self.peek() {
                Some('\\') => {
                    self.advance();
                    if is_qq {
                        match self.advance() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('r') => s.push('\r'),
                            Some('\\') => s.push('\\'),
                            Some(c @ '0'..='7') => {
                                let mut oct = String::new();
                                oct.push(c);
                                for _ in 0..2 {
                                    match self.peek() {
                                        Some(d) if ('0'..='7').contains(&d) => {
                                            oct.push(self.advance().unwrap());
                                        }
                                        _ => break,
                                    }
                                }
                                let val = u32::from_str_radix(&oct, 8).unwrap();
                                let ch = char::from_u32(val).ok_or_else(|| {
                                    self.syntax_err("Invalid octal escape", self.line)
                                })?;
                                s.push(ch);
                            }
                            Some('a') => s.push('\x07'),
                            Some('b') => s.push('\x08'),
                            Some('f') => s.push('\x0C'),
                            Some('e') => s.push('\x1B'),
                            Some('$') => s.push(LITERAL_DOLLAR_IN_DQUOTE),
                            Some('c') => {
                                let ch = self.advance().ok_or_else(|| {
                                    self.syntax_err("Unterminated \\c escape", self.line)
                                })?;
                                s.push(char::from(ch.to_ascii_uppercase() as u8 ^ 0x40));
                            }
                            Some('o') if self.peek() == Some('{') => {
                                self.advance();
                                let oct = self.read_while(|c| c != '}');
                                if self.peek() != Some('}') {
                                    return Err(self.syntax_err(
                                        "Unterminated \\o{...} in qq string",
                                        self.line,
                                    ));
                                }
                                self.advance();
                                if oct.is_empty() {
                                    return Err(
                                        self.syntax_err("Empty \\o{} in qq string", self.line)
                                    );
                                }
                                let val = u32::from_str_radix(&oct, 8).map_err(|_| {
                                    self.syntax_err("Invalid octal digits in \\o{...}", self.line)
                                })?;
                                let c = char::from_u32(val).ok_or_else(|| {
                                    self.syntax_err(
                                        "Invalid Unicode scalar value in \\o{...}",
                                        self.line,
                                    )
                                })?;
                                s.push(c);
                            }
                            Some('u') if self.peek() == Some('{') => {
                                self.advance();
                                let hex = self.read_while(|c| c != '}');
                                if self.peek() != Some('}') {
                                    return Err(self.syntax_err(
                                        "Unterminated \\u{...} in qq string",
                                        self.line,
                                    ));
                                }
                                self.advance();
                                if hex.is_empty() {
                                    return Err(
                                        self.syntax_err("Empty \\u{} in qq string", self.line)
                                    );
                                }
                                let val = u32::from_str_radix(&hex, 16).map_err(|_| {
                                    self.syntax_err("Invalid hex digits in \\u{...}", self.line)
                                })?;
                                let c = char::from_u32(val).ok_or_else(|| {
                                    self.syntax_err(
                                        "Invalid Unicode scalar value in \\u{...}",
                                        self.line,
                                    )
                                })?;
                                s.push(c);
                            }
                            Some('N') if self.peek() == Some('{') => {
                                self.advance();
                                let name = self.read_while(|c| c != '}');
                                if self.peek() != Some('}') {
                                    return Err(self.syntax_err(
                                        "Unterminated \\N{...} in qq string",
                                        self.line,
                                    ));
                                }
                                self.advance();
                                if name.is_empty() {
                                    return Err(
                                        self.syntax_err("Empty \\N{} in qq string", self.line)
                                    );
                                }
                                let c = parse_unicode_name(&name).ok_or_else(|| {
                                    self.syntax_err(
                                        format!("Unknown Unicode character name: {name}"),
                                        self.line,
                                    )
                                })?;
                                s.push(c);
                            }
                            Some('x') => {
                                if self.peek() == Some('{') {
                                    self.advance();
                                    let hex = self.read_while(|c| c != '}');
                                    if self.peek() != Some('}') {
                                        return Err(self.syntax_err(
                                            "Unterminated \\x{...} in qq string",
                                            self.line,
                                        ));
                                    }
                                    self.advance();
                                    if hex.is_empty() {
                                        return Err(
                                            self.syntax_err("Empty \\x{} in qq string", self.line)
                                        );
                                    }
                                    let val = u32::from_str_radix(&hex, 16).map_err(|_| {
                                        self.syntax_err("Invalid hex digits in \\x{...}", self.line)
                                    })?;
                                    let c = char::from_u32(val).ok_or_else(|| {
                                        self.syntax_err(
                                            "Invalid Unicode scalar value in \\x{...}",
                                            self.line,
                                        )
                                    })?;
                                    s.push(c);
                                } else {
                                    let mut hex = String::new();
                                    for _ in 0..2 {
                                        match self.peek() {
                                            Some(c) if c.is_ascii_hexdigit() => {
                                                hex.push(self.advance().unwrap());
                                            }
                                            _ => break,
                                        }
                                    }
                                    if hex.is_empty() {
                                        s.push('\0');
                                    } else if let Ok(val) = u32::from_str_radix(&hex, 16) {
                                        if let Some(c) = char::from_u32(val) {
                                            s.push(c);
                                        } else {
                                            return Err(self.syntax_err(
                                                "Invalid code point in \\x escape",
                                                self.line,
                                            ));
                                        }
                                    }
                                }
                            }
                            Some(c) if c == close && depth == 1 => s.push(close),
                            Some(c) => {
                                s.push('\\');
                                s.push(c);
                            }
                            None => {
                                return Err(
                                    self.syntax_err("Unterminated qq(...) string", self.line)
                                );
                            }
                        }
                    } else {
                        match self.advance() {
                            Some(c) if c == close && depth == 1 => s.push(close),
                            Some(c) => {
                                s.push('\\');
                                s.push(c);
                            }
                            None => {
                                return Err(
                                    self.syntax_err("Unterminated q(...) string", self.line)
                                );
                            }
                        }
                    }
                }
                Some(c) if c == open => {
                    self.advance();
                    depth += 1;
                    s.push(open);
                }
                Some(c) if c == close => {
                    self.advance();
                    if depth == 1 {
                        break;
                    }
                    depth -= 1;
                    s.push(close);
                }
                Some(c) => {
                    self.advance();
                    s.push(c);
                }
                None => {
                    return Err(self.syntax_err("Unterminated q/qq bracketed string", self.line));
                }
            }
        }
        Ok(s)
    }

    fn read_regex(&mut self) -> PerlResult<Token> {
        self.advance(); // consume opening /
        let mut pattern = String::new();
        loop {
            match self.advance() {
                Some('\\') => {
                    pattern.push('\\');
                    if let Some(c) = self.advance() {
                        pattern.push(c);
                    }
                }
                Some('/') => break,
                Some(c) => pattern.push(c),
                None => return Err(self.syntax_err("Unterminated regex", self.line)),
            }
        }
        let flags = self.read_while(|c| REGEX_FLAG_CHARS.contains(c));
        Ok(Token::Regex(pattern, flags, '/'))
    }

    fn read_qw(&mut self) -> PerlResult<Token> {
        // Already consumed 'qw', now expect delimiter
        self.skip_whitespace_only();
        let open = self
            .advance()
            .ok_or_else(|| self.syntax_err("Expected delimiter after qw", self.line))?;
        let close = match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '<' => '>',
            c => c,
        };
        let mut words = Vec::new();
        if matches!(open, '(' | '[' | '{' | '<') {
            // Perl balances nested delimiters in `qw( ... )` / `qw[ ... ]` / … so
            // `qw( (SV*)pWARN_ALL )` is one word (core `B.pm` line 88).
            let mut depth: usize = 1;
            let mut buf = String::new();
            loop {
                match self.peek() {
                    None => {
                        return Err(self.syntax_err("Unterminated qw()", self.line));
                    }
                    Some(c) if depth == 1 && c.is_whitespace() => {
                        self.advance();
                        if !buf.is_empty() {
                            words.push(buf.clone());
                            buf.clear();
                        }
                        while self.peek().is_some_and(|c| c.is_whitespace()) {
                            self.advance();
                        }
                    }
                    Some(c) if c == close && depth == 1 => {
                        self.advance();
                        if !buf.is_empty() {
                            words.push(buf);
                        }
                        break;
                    }
                    Some(c) if c == open => {
                        depth += 1;
                        buf.push(self.advance().unwrap());
                    }
                    Some(c) if c == close => {
                        // `depth == 1 && close` is handled above (final qw delimiter).
                        debug_assert!(depth >= 2);
                        depth -= 1;
                        buf.push(self.advance().unwrap());
                    }
                    Some(_) => {
                        buf.push(self.advance().unwrap());
                    }
                }
            }
            return Ok(Token::QW(words));
        }
        loop {
            // Skip whitespace inside qw
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.advance();
                } else {
                    break;
                }
            }
            if self.peek() == Some(close) {
                self.advance();
                break;
            }
            if self.peek().is_none() {
                return Err(self.syntax_err("Unterminated qw()", self.line));
            }
            let word = self.read_while(|c| !c.is_whitespace() && c != close);
            if !word.is_empty() {
                words.push(word);
            }
        }
        Ok(Token::QW(words))
    }

    fn read_heredoc_tag(&mut self) -> PerlResult<(String, bool, bool)> {
        self.read_heredoc_tag_inner(false)
    }

    fn read_heredoc_tag_inner(&mut self, indented: bool) -> PerlResult<(String, bool, bool)> {
        // We've consumed '<<'. Now figure out the tag.
        // Returns (tag, interpolate, indented).
        let quoted;
        let tag;
        match self.peek() {
            Some('\'') => {
                self.advance();
                tag = self.read_while(|c| c != '\'');
                self.advance(); // closing quote
                quoted = false; // no interpolation
            }
            Some('"') => {
                self.advance();
                tag = self.read_while(|c| c != '"');
                self.advance();
                quoted = true;
            }
            Some('~') => {
                self.advance(); // indented heredoc
                return self.read_heredoc_tag_inner(true); // recurse with indented=true
            }
            _ => {
                tag = self.read_while(|c| c.is_alphanumeric() || c == '_');
                quoted = true;
            }
        }
        Ok((tag, quoted, indented))
    }

    fn read_heredoc_body(&mut self, tag: &str, indented: bool) -> PerlResult<String> {
        // Read until we find a line that is exactly the tag (or, for indented heredocs,
        // a line whose trimmed content equals the tag).
        let mut lines: Vec<String> = Vec::new();
        // First, skip to end of current line
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                self.advance();
                break;
            }
            self.advance();
        }
        let mut terminator_indent: Option<usize> = None;
        loop {
            let _line_start = self.pos;
            let line = self.read_while(|c| c != '\n');
            if line.trim() == tag {
                // For indented heredocs, the terminator's leading whitespace determines
                // how much to strip from all body lines.
                if indented {
                    terminator_indent = Some(line.len() - line.trim_start().len());
                }
                break;
            }
            lines.push(line);
            if self.peek() == Some('\n') {
                self.advance();
            } else if self.pos >= self.input.len() {
                return Err(self.syntax_err(
                    format!("Unterminated heredoc (looking for '{tag}')"),
                    self.line,
                ));
            }
        }
        if self.peek() == Some('\n') {
            self.advance();
        }
        // For indented heredocs (<<~), strip leading whitespace from each line,
        // up to the amount of indentation on the terminator line.
        if indented {
            let strip = terminator_indent.unwrap_or(0);
            let mut body = String::new();
            for line in lines {
                let ws_count = line.len() - line.trim_start().len();
                let to_strip = ws_count.min(strip);
                body.push_str(&line[to_strip..]);
                body.push('\n');
            }
            Ok(body)
        } else {
            let mut body = String::new();
            for line in lines {
                body.push_str(&line);
                body.push('\n');
            }
            Ok(body)
        }
    }

    fn read_identifier(&mut self) -> String {
        self.read_while(|c| c.is_alphanumeric() || c == '_')
    }

    /// `Foo::Bar::Baz` after the leading sigil.
    fn read_package_qualified_identifier(&mut self) -> String {
        let mut s = self.read_identifier();
        while self.peek() == Some(':') && self.input.get(self.pos + 1) == Some(&':') {
            self.advance();
            self.advance();
            s.push_str("::");
            s.push_str(&self.read_identifier());
        }
        s
    }

    /// Body lines for `format N =` … `.` (excluding the closing `.` line).
    fn read_format_body(&mut self) -> PerlResult<Vec<String>> {
        while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
            self.advance();
        }
        if self.peek() == Some('\n') {
            self.advance();
        }
        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            while let Some(c) = self.peek() {
                if c == '\n' {
                    self.advance();
                    break;
                }
                if c == '\r' {
                    self.advance();
                    if self.peek() == Some('\n') {
                        self.advance();
                    }
                    break;
                }
                line.push(c);
                self.advance();
            }
            if line.trim() == "." {
                break;
            }
            lines.push(line);
            if self.peek().is_none() {
                return Err(self.syntax_err(
                    "Unterminated format (expected '.' on its own line before end of file)",
                    self.line,
                ));
            }
        }
        Ok(lines)
    }

    fn read_variable_name(&mut self) -> String {
        // Handle special vars like $_, $!, $0, $/, $^I, etc.
        match self.peek() {
            // Second `$` in `$$_{` — with leading `$` already consumed, we have `$` `_` `{` → `$_` then `{`.
            Some('$')
                if self.input.get(self.pos + 1) == Some(&'_')
                    && self.input.get(self.pos + 2) == Some(&'{') =>
            {
                self.advance(); // second $
                self.advance(); // `_` of `$_`
                "_".to_string()
            }
            // `$::{$key}` / `$::Foo` — stash access (`%::`) and package names rooted at `::` (Perl `$::` ≡ main stash).
            Some(':') if self.input.get(self.pos + 1) == Some(&':') => {
                self.advance();
                self.advance();
                let mut s = "::".to_string();
                if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
                    s.push_str(&self.read_identifier());
                }
                while self.peek() == Some(':') && self.input.get(self.pos + 1) == Some(&':') {
                    self.advance();
                    self.advance();
                    s.push_str("::");
                    s.push_str(&self.read_identifier());
                }
                s
            }
            Some(c) if c.is_alphabetic() || c == '_' => {
                let ident = self.read_package_qualified_identifier();
                // `$_<`, `$_<<`, … — outer topic (stryke extension); only for bare `_`.
                if ident == "_" {
                    let mut lts = String::new();
                    while self.peek() == Some('<') {
                        self.advance();
                        lts.push('<');
                    }
                    if !lts.is_empty() {
                        return format!("_{}", lts);
                    }
                }
                ident
            }
            Some('^') => {
                self.advance();
                // Perl `$^I`, `$^O`, … — caret plus one letter (or `^` alone).
                if self.peek().is_some_and(|c| c.is_alphabetic()) {
                    let c2 = self.advance().unwrap();
                    format!("^{}", c2)
                } else {
                    "^".to_string()
                }
            }
            // `${name}` — must run before the punctuation branch (`{` is also listed there).
            Some('{') => {
                self.advance(); // {
                let name = self.read_while(|c| c != '}');
                if self.peek() == Some('}') {
                    self.advance();
                }
                name
            }
            // Perl `$#name` — last index of `@name` (scalar name stored as `#name`).
            Some('#') => {
                self.advance();
                if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
                    let mut name = String::from("#");
                    name.push_str(&self.read_package_qualified_identifier());
                    name
                } else {
                    "#".to_string()
                }
            }
            Some(c) if "!@$&*+;',\"\\|?/<>.0123456789~%-=()[]{}".contains(c) => {
                self.advance();
                c.to_string()
            }
            _ => String::new(),
        }
    }

    /// `${$name}` / `${$Foo::bar}` — when the braced body is a plain scalar `$identifier`, Perl treats it
    /// like `$$name` (scalar deref). The naive lexer otherwise yields a bogus [`Token::ScalarVar`] name
    /// containing a leading `$` (e.g. Try::Tiny's `${$code_ref}`).
    fn braced_body_symbolic_scalar_deref_name(body: &str) -> Option<&str> {
        let body = body.trim();
        let rest = body.strip_prefix('$')?;
        if rest.is_empty() {
            return None;
        }
        let mut chars = rest.chars();
        let c0 = chars.next()?;
        if !(c0.is_alphabetic() || c0 == '_') {
            return None;
        }
        for c in chars {
            if !(c.is_alphanumeric() || c == '_' || c == ':') {
                return None;
            }
        }
        Some(rest)
    }

    pub fn next_token(&mut self) -> PerlResult<Token> {
        self.skip_whitespace_and_comments();

        if self.pos >= self.input.len() {
            return Ok(Token::Eof);
        }

        let ch = self.input[self.pos];
        match ch {
            // Variables
            '$' => {
                self.advance();
                // `$$foo` — symbolic scalar deref (Perl `${$foo}`-style lookup)
                if self.peek() == Some('$') {
                    // `$$_{` — Perl parses as `$_->{...}` (implicit arrow on `$_`), not `$$` PID + `_`.
                    let is_dollar_under_brace = self.input.get(self.pos + 1) == Some(&'_')
                        && self.input.get(self.pos + 2) == Some(&'{');
                    if !is_dollar_under_brace {
                        self.advance();
                        if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
                            let name = self.read_identifier();
                            self.last_was_term = true;
                            return Ok(Token::DerefScalarVar(name));
                        }
                        // `$$` — process id (Perl `$$`)
                        self.last_was_term = true;
                        return Ok(Token::ScalarVar("$$".to_string()));
                    }
                }
                let name = self.read_variable_name();
                if name.is_empty() {
                    return Err(self.syntax_err("Expected variable name after $", self.line));
                }
                self.last_was_term = true;
                if let Some(tail) = Self::braced_body_symbolic_scalar_deref_name(&name) {
                    return Ok(Token::DerefScalarVar(tail.to_string()));
                }
                Ok(Token::ScalarVar(name))
            }
            '@' => {
                self.advance();
                if self.peek() == Some('-') {
                    self.advance();
                    self.last_was_term = true;
                    return Ok(Token::ArrayVar("-".to_string()));
                }
                if self.peek() == Some('+') {
                    self.advance();
                    self.last_was_term = true;
                    return Ok(Token::ArrayVar("+".to_string()));
                }
                if self.peek() == Some('^')
                    && self
                        .input
                        .get(self.pos + 1)
                        .is_some_and(|c| c.is_alphabetic() || *c == '_')
                {
                    self.advance();
                    let name = format!("^{}", self.read_package_qualified_identifier());
                    self.last_was_term = true;
                    return Ok(Token::ArrayVar(name));
                }
                if self.peek() == Some('_') || self.peek().is_some_and(|c| c.is_alphabetic()) {
                    let name = self.read_package_qualified_identifier();
                    self.last_was_term = true;
                    return Ok(Token::ArrayVar(name));
                }
                self.last_was_term = false;
                Ok(Token::ArrayAt)
            }
            '%' if !self.last_was_term => {
                self.advance();
                // `%+` — named regex captures (Perl special hash)
                if self.peek() == Some('+') {
                    self.advance();
                    self.last_was_term = true;
                    return Ok(Token::HashVar("+".to_string()));
                }
                if self.peek() == Some('^')
                    && self
                        .input
                        .get(self.pos + 1)
                        .is_some_and(|c| c.is_alphabetic() || *c == '_')
                {
                    self.advance();
                    let name = format!("^{}", self.read_package_qualified_identifier());
                    self.last_was_term = true;
                    return Ok(Token::HashVar(name));
                }
                if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
                    let name = self.read_package_qualified_identifier();
                    self.last_was_term = true;
                    return Ok(Token::HashVar(name));
                }
                self.last_was_term = false;
                Ok(Token::HashPercent)
            }

            // Numbers
            '0'..='9' => {
                let tok = self.read_number()?;
                self.last_was_term = true;
                Ok(tok)
            }

            // Strings
            '\'' => {
                let tok = self.read_single_quoted_string()?;
                self.last_was_term = true;
                Ok(tok)
            }
            '"' => {
                let tok = self.read_double_quoted_string()?;
                self.last_was_term = true;
                Ok(tok)
            }

            // Backtick — Perl `` `cmd` `` (qx), not a plain double-quoted string
            '`' => {
                self.advance();
                let cmd = self.read_escaped_until('`')?;
                self.last_was_term = true;
                Ok(Token::BacktickString(cmd))
            }

            // Regex or division
            '/' => {
                if !self.last_was_term {
                    let tok = self.read_regex()?;
                    self.last_was_term = true;
                    return Ok(tok);
                }
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::DivAssign);
                }
                if self.peek() == Some('/') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::DefinedOrAssign);
                    }
                    self.last_was_term = false;
                    return Ok(Token::DefinedOr);
                }
                self.last_was_term = false;
                Ok(Token::Slash)
            }

            // Operators and punctuation
            '+' => {
                self.advance();
                if self.peek() == Some('+') {
                    self.advance();
                    // Whether it was term depends on context
                    return Ok(Token::Increment);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::PlusAssign);
                }
                self.last_was_term = false;
                Ok(Token::Plus)
            }
            '-' => {
                self.advance();
                // File test operators: -e, -f, -d, etc.
                if !self.last_was_term {
                    if let Some(c) = self.peek() {
                        if "efdlpSszrwxoRWXOBCTMAgut".contains(c)
                            && self.peek_at(1).is_none_or(|n| {
                                n.is_whitespace()
                                    || n == '$'
                                    || n == '\''
                                    || n == '"'
                                    || n == '('
                                    || n == ')'
                                    || n == '}'
                                    || n == ';'
                                    || n == ','
                            })
                        {
                            self.advance();
                            self.last_was_term = false;
                            return Ok(Token::FileTest(c));
                        }
                    }
                }
                if self.peek() == Some('-') {
                    self.advance();
                    return Ok(Token::Decrement);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::MinusAssign);
                }
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::ThreadArrowLast);
                    }
                    self.last_was_term = false;
                    return Ok(Token::Arrow);
                }
                self.last_was_term = false;
                Ok(Token::Minus)
            }
            '*' => {
                self.advance();
                if self.peek() == Some('*') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::PowAssign);
                    }
                    self.last_was_term = false;
                    return Ok(Token::Power);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::MulAssign);
                }
                self.last_was_term = false;
                Ok(Token::Star)
            }
            '%' => {
                // Only reached when last_was_term is true (hash sigil handled above)
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::ModAssign);
                }
                self.last_was_term = false;
                Ok(Token::Percent)
            }
            '.' => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    if self.peek() == Some('.') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::RangeExclusive);
                    }
                    self.last_was_term = false;
                    return Ok(Token::Range);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::DotAssign);
                }
                self.last_was_term = false;
                Ok(Token::Dot)
            }
            '=' => {
                let eq_pos = self.pos;
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::NumEq);
                }
                if self.peek() == Some('~') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::BindMatch);
                }
                if self.peek() == Some('>') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::FatArrow);
                }
                // POD: =head1 etc — only when `=` begins the line (after optional whitespace).
                if self.peek().is_some_and(|c| c.is_alphabetic())
                    && self.at_line_start_for_pod(eq_pos)
                {
                    // Skip POD
                    loop {
                        let line = self.read_while(|c| c != '\n');
                        if self.peek() == Some('\n') {
                            self.advance();
                        }
                        if line.starts_with("=cut") || self.pos >= self.input.len() {
                            break;
                        }
                    }
                    return self.next_token();
                }
                self.last_was_term = false;
                Ok(Token::Assign)
            }
            '!' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::NumNe);
                }
                if self.peek() == Some('~') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::BindNotMatch);
                }
                self.last_was_term = false;
                Ok(Token::LogNot)
            }
            '<' => {
                self.advance();
                let after_lt = self.pos;
                // Readline `<$fh>` (scalar handle) — must come before `<IDENT>` / numeric `<`.
                if self.peek() == Some('$') {
                    self.advance();
                    let name = self.read_variable_name();
                    if !name.is_empty() && self.peek() == Some('>') {
                        self.advance();
                        self.last_was_term = true;
                        return Ok(Token::ReadLine(name));
                    }
                    self.pos = after_lt;
                }
                // Diamond operator <> or <STDIN>
                if self.peek() == Some('>') {
                    self.advance();
                    self.last_was_term = true;
                    return Ok(Token::Diamond);
                }
                if self.peek().is_some_and(|c| c.is_uppercase()) {
                    let name = self.read_identifier();
                    if self.peek() == Some('>') {
                        self.advance();
                        self.last_was_term = true;
                        return Ok(Token::ReadLine(name));
                    }
                    // Not a readline, put back — this is tricky, we'll handle as less-than
                    // followed by ident. For simplicity, return the ident separately.
                    self.last_was_term = false;
                    return Ok(Token::NumLt);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::Spaceship);
                    }
                    self.last_was_term = false;
                    return Ok(Token::NumLe);
                }
                if self.peek() == Some('<') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::ShiftLeftAssign);
                    }
                    // `<<` — binary shift after a complete term (`1 << 4`, `"x" << 2`); heredoc when a
                    // term is expected (`print <<EOF`, `my $x = <<EOF`, after `.` / `,` / `(` …).
                    if self.last_was_term {
                        self.last_was_term = false;
                        return Ok(Token::ShiftLeft);
                    }
                    let (tag, interpolate, indented) = self.read_heredoc_tag()?;
                    let body = self.read_heredoc_body(&tag, indented)?;
                    self.last_was_term = true;
                    return Ok(Token::HereDoc(tag, body, interpolate));
                }
                self.last_was_term = false;
                Ok(Token::NumLt)
            }
            '>' => {
                self.advance();
                if self.peek() == Some('{') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::ArrowBrace);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::NumGe);
                }
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::ShiftRightAssign);
                    }
                    self.last_was_term = false;
                    return Ok(Token::ShiftRight);
                }
                self.last_was_term = false;
                Ok(Token::NumGt)
            }
            '&' => {
                self.advance();
                if self.peek() == Some('&') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::AndAssign);
                    }
                    self.last_was_term = false;
                    return Ok(Token::LogAnd);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::BitAndAssign);
                }
                self.last_was_term = false;
                Ok(Token::BitAnd)
            }
            '|' => {
                self.advance();
                if self.peek() == Some('|') {
                    self.advance();
                    if self.peek() == Some('=') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::OrAssign);
                    }
                    self.last_was_term = false;
                    return Ok(Token::LogOr);
                }
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::BitOrAssign);
                }
                if self.peek() == Some('>') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::PipeForward);
                }
                self.last_was_term = false;
                Ok(Token::BitOr)
            }
            '^' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::XorAssign);
                }
                self.last_was_term = false;
                Ok(Token::BitXor)
            }
            '~' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    if self.peek() == Some('>') {
                        self.advance();
                        self.last_was_term = false;
                        return Ok(Token::ThreadArrowLast);
                    }
                    self.last_was_term = false;
                    return Ok(Token::ThreadArrow);
                }
                self.last_was_term = false;
                Ok(Token::BitNot)
            }
            '?' => {
                self.advance();
                self.last_was_term = false;
                Ok(Token::Question)
            }
            ':' => {
                self.advance();
                if self.peek() == Some(':') {
                    self.advance();
                    self.last_was_term = false;
                    return Ok(Token::PackageSep);
                }
                self.last_was_term = false;
                Ok(Token::Colon)
            }
            '\\' => {
                self.advance();
                // Backslash-newline: line continuation (shell-style)
                // Don't increment line — continued line is logically part of the same line
                if self.peek() == Some('\n') {
                    self.pos += 1; // skip newline without incrementing self.line
                    return self.next_token();
                }
                self.last_was_term = false;
                Ok(Token::Backslash)
            }
            ',' => {
                self.advance();
                self.last_was_term = false;
                Ok(Token::Comma)
            }
            ';' => {
                self.advance();
                self.last_was_term = false;
                Ok(Token::Semicolon)
            }
            '(' => {
                self.advance();
                self.last_was_term = false;
                Ok(Token::LParen)
            }
            ')' => {
                self.advance();
                self.last_was_term = true;
                Ok(Token::RParen)
            }
            '[' => {
                self.advance();
                self.last_was_term = false;
                Ok(Token::LBracket)
            }
            ']' => {
                self.advance();
                self.last_was_term = true;
                Ok(Token::RBracket)
            }
            '{' => {
                self.advance();
                self.last_was_term = false;
                Ok(Token::LBrace)
            }
            '}' => {
                self.advance();
                self.last_was_term = true;
                Ok(Token::RBrace)
            }

            // Identifiers and keywords
            c if c.is_alphabetic() || c == '_' => {
                let ident = self.read_identifier();

                // Special multi-char constructs
                match ident.as_str() {
                    "format" => {
                        self.skip_whitespace_and_comments();
                        let fname = self.read_package_qualified_identifier();
                        self.skip_whitespace_and_comments();
                        if self.peek() != Some('=') {
                            return Err(
                                self.syntax_err("Expected '=' after format name", self.line)
                            );
                        }
                        self.advance();
                        let lines = self.read_format_body()?;
                        self.last_was_term = false;
                        return Ok(Token::FormatDecl { name: fname, lines });
                    }
                    "qw" => {
                        // `qw` followed by `=>` is an autoquoted hash key, not qw().
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(c) = self.peek() {
                            if c == '=' && self.peek_at(1) == Some('>') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                            if matches!(c, ';' | ',' | ')' | ']' | '}' | '\n') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        self.pos = start_pos; // restore for read_qw
                        let tok = self.read_qw()?;
                        self.last_was_term = true;
                        return Ok(tok);
                    }
                    "qq" | "q" => {
                        // `q` / `qq` followed by `=>` is an autoquoted hash key, not a quote operator.
                        // Also treat as identifier if followed by terminators like `;`, `,`, `)`, etc.
                        // Must check AFTER skipping whitespace to handle `q => 5`.
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(c) = self.peek() {
                            // `=` followed by `>` is fat comma — `q` is a bareword key
                            if c == '=' && self.peek_at(1) == Some('>') {
                                self.pos = start_pos; // restore position
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                            // Other terminators: `q` is an identifier
                            if matches!(c, ';' | ',' | ')' | ']' | '}' | '\n') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        let delim = self.advance().ok_or_else(|| {
                            self.syntax_err("Expected delimiter after q/qq", self.line)
                        })?;
                        let close = match delim {
                            '(' => ')',
                            '[' => ']',
                            '{' => '}',
                            '<' => '>',
                            c => c,
                        };
                        let s = if matches!(delim, '(' | '[' | '{' | '<') {
                            self.read_q_qq_balanced_body(delim, close, ident == "qq")?
                        } else {
                            self.read_escaped_until(close)?
                        };
                        self.last_was_term = true;
                        if ident == "qq" {
                            return Ok(Token::DoubleString(s));
                        }
                        return Ok(Token::SingleString(s));
                    }
                    "qx" => {
                        // `qx` followed by `=>` is an autoquoted hash key.
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(c) = self.peek() {
                            if c == '=' && self.peek_at(1) == Some('>') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                            if matches!(c, ';' | ',' | ')' | ']' | '}' | '\n') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        let delim = self.advance().ok_or_else(|| {
                            self.syntax_err("Expected delimiter after qx", self.line)
                        })?;
                        let close = match delim {
                            '(' => ')',
                            '[' => ']',
                            '{' => '}',
                            '<' => '>',
                            c => c,
                        };
                        let s = self.read_escaped_until(close)?;
                        self.last_was_term = true;
                        return Ok(Token::BacktickString(s));
                    }
                    "qr" => {
                        // `qr` followed by `=>` is an autoquoted hash key.
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(c) = self.peek() {
                            if c == '=' && self.peek_at(1) == Some('>') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                            if matches!(c, ';' | ',' | ')' | ']' | '}' | '\n') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        let delim = self.advance().ok_or_else(|| {
                            self.syntax_err("Expected delimiter after qr", self.line)
                        })?;
                        let close = match delim {
                            '(' => ')',
                            '[' => ']',
                            '{' => '}',
                            '<' => '>',
                            c => c,
                        };
                        let pattern = self.read_escaped_until(close)?;
                        let flags = self.read_while(|c| REGEX_FLAG_CHARS.contains(c));
                        self.last_was_term = true;
                        return Ok(Token::Regex(pattern, flags, delim));
                    }
                    "m" => {
                        // `m` followed by terminators is a bareword, not match operator.
                        // Must check AFTER skipping whitespace to handle `m => "val"`.
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(d) = self.peek() {
                            if d == '=' && self.peek_at(1) == Some('>') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                            if matches!(d, ';' | ',' | ')' | ']' | '}' | '>' | ':' | '\n') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        self.pos = start_pos;
                        // m/pattern/flags — try parsing as regex, but backtrack if
                        // unterminated (handles thread stages where `/m/` is a grep filter)
                        if self.suppress_m_regex == 0 {
                            if let Some(delim) = self.peek() {
                                if !delim.is_alphanumeric() && delim != '_' {
                                    // Save state for backtracking
                                    let saved_pos = self.pos;
                                    let saved_line = self.line;
                                    self.advance(); // consume delimiter
                                    let close = match delim {
                                        '(' => ')',
                                        '[' => ']',
                                        '{' => '}',
                                        '<' => '>',
                                        c => c,
                                    };
                                    let mut pattern = String::new();
                                    let mut terminated = true;
                                    loop {
                                        match self.advance() {
                                            Some('\\') => {
                                                pattern.push('\\');
                                                if let Some(c) = self.advance() {
                                                    pattern.push(c);
                                                }
                                            }
                                            Some(c) if c == close => break,
                                            Some(c) if c == '\n' && close == '/' => {
                                                // Newline before closing / — not a valid m//
                                                terminated = false;
                                                break;
                                            }
                                            Some(c) => pattern.push(c),
                                            None => {
                                                return Err(self.syntax_err(
                                                    "Search pattern not terminated",
                                                    saved_line,
                                                ));
                                            }
                                        }
                                    }
                                    if terminated {
                                        let flags =
                                            self.read_while(|c| REGEX_FLAG_CHARS.contains(c));
                                        self.last_was_term = true;
                                        return Ok(Token::Regex(pattern, flags, delim));
                                    }
                                    // Newline before closing / — backtrack and treat `m` as identifier
                                    self.pos = saved_pos;
                                    self.line = saved_line;
                                }
                            }
                        }
                        // Just the identifier 'm'
                        self.last_was_term = true;
                        return Ok(Token::Ident(ident));
                    }
                    "s" => {
                        // `s` followed by terminators is a bareword, not substitution.
                        // Must check AFTER skipping whitespace to handle `s => "val"`.
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(d) = self.peek() {
                            if d == '=' && self.peek_at(1) == Some('>') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                            if matches!(d, ';' | ',' | ')' | ']' | '}' | '>' | ':' | '\n') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        self.pos = start_pos;
                        // s/pattern/replacement/flags
                        if let Some(delim) = self.peek() {
                            if !delim.is_alphanumeric() && delim != '_' && delim != ' ' {
                                self.advance();
                                let close = match delim {
                                    '(' => ')',
                                    '[' => ']',
                                    '{' => '}',
                                    '<' => '>',
                                    c => c,
                                };
                                let mut pattern = String::new();
                                loop {
                                    match self.advance() {
                                        Some('\\') => {
                                            pattern.push('\\');
                                            if let Some(c) = self.advance() {
                                                pattern.push(c);
                                            }
                                        }
                                        Some(c) if c == close => break,
                                        Some(c) => pattern.push(c),
                                        None => {
                                            return Err(self.syntax_err(
                                                "Unterminated s/// pattern",
                                                self.line,
                                            ))
                                        }
                                    }
                                }
                                // For paired delimiters, read the opening of the replacement part
                                if "([{<".contains(delim) {
                                    self.skip_whitespace_only();
                                    let open2 = self.advance().unwrap_or(delim);
                                    let close = match open2 {
                                        '(' => ')',
                                        '[' => ']',
                                        '{' => '}',
                                        '<' => '>',
                                        c => c,
                                    };
                                    let replacement = self.read_escaped_until(close)?;
                                    let flags = self.read_while(|c| REGEX_FLAG_CHARS.contains(c));
                                    self.last_was_term = true;
                                    // Encode as special token — parser will decode
                                    // Format: \x00s\x00pattern\x00replacement\x00flags\x00delim
                                    return Ok(Token::Ident(format!(
                                        "\x00s\x00{}\x00{}\x00{}\x00{}",
                                        pattern, replacement, flags, delim
                                    )));
                                }
                                let replacement = self.read_escaped_until(close)?;
                                let flags = self.read_while(|c| REGEX_FLAG_CHARS.contains(c));
                                self.last_was_term = true;
                                return Ok(Token::Ident(format!(
                                    "\x00s\x00{}\x00{}\x00{}\x00{}",
                                    pattern, replacement, flags, delim
                                )));
                            }
                        }
                        self.last_was_term = true;
                        return Ok(Token::Ident(ident));
                    }
                    "tr" | "y" => {
                        // After `::`, treat as package-qualified identifier, not transliteration.
                        // e.g. `Foo::y(...)` is a function call, not `y///`.
                        if self.pos >= ident.len() + 2 {
                            let prev_start = self.pos - ident.len() - 2;
                            if self.input.get(prev_start) == Some(&':')
                                && self.input.get(prev_start + 1) == Some(&':')
                            {
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        // `tr` / `y` followed by terminators is a bareword, not transliteration.
                        // Check BEFORE skipping whitespace to catch newlines (implicit semicolon).
                        if let Some(d) = self.peek() {
                            if matches!(d, ';' | ',' | ')' | ']' | '}' | '>' | ':' | '\n') {
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        } else {
                            self.last_was_term = true;
                            return Ok(Token::Ident(ident));
                        }
                        // Now skip whitespace to check for `=>` or `=`
                        let start_pos = self.pos;
                        self.skip_whitespace_only();
                        if let Some(d) = self.peek() {
                            // `=` alone (not `==` comparison) means assignment — y is an identifier
                            if d == '=' && self.peek_at(1) != Some('=') {
                                self.pos = start_pos;
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        self.pos = start_pos;
                        // Check for function signature pattern: y(...) { — this is `fn y`, not tr
                        if self.peek() == Some('(') {
                            // Scan ahead to see if there's ) followed by {
                            let scan_pos = self.pos;
                            self.advance(); // skip (
                            let mut depth = 1;
                            while depth > 0 {
                                match self.peek() {
                                    Some('(') => {
                                        self.advance();
                                        depth += 1;
                                    }
                                    Some(')') => {
                                        self.advance();
                                        depth -= 1;
                                    }
                                    Some(_) => {
                                        self.advance();
                                    }
                                    None => break,
                                }
                            }
                            self.skip_whitespace_only();
                            let is_func_def = self.peek() == Some('{');
                            self.pos = scan_pos;
                            if is_func_def {
                                self.last_was_term = true;
                                return Ok(Token::Ident(ident));
                            }
                        }
                        // tr/from/to/flags
                        if let Some(delim) = self.peek() {
                            if !delim.is_alphanumeric() && delim != '_' && delim != ' ' {
                                self.advance();
                                let close = match delim {
                                    '(' => ')',
                                    '[' => ']',
                                    '{' => '}',
                                    '<' => '>',
                                    c => c,
                                };
                                let from = self.read_escaped_until(close)?;
                                // For paired delimiters
                                if "([{<".contains(delim) {
                                    self.skip_whitespace_only();
                                    self.advance(); // open second pair
                                }
                                let to = self.read_escaped_until(close)?;
                                let flags = self.read_while(|c| "cdsr".contains(c));
                                self.last_was_term = true;
                                return Ok(Token::Ident(format!(
                                    "\x00tr\x00{}\x00{}\x00{}\x00{}",
                                    from, to, flags, delim
                                )));
                            }
                        }
                        self.last_was_term = true;
                        return Ok(Token::Ident(ident));
                    }
                    _ => {}
                }

                // Fat arrow lookahead: ident followed by => is a string
                let saved_pos2 = self.pos;
                self.skip_whitespace_and_comments();
                if self.peek() == Some('=') && self.peek_at(1) == Some('>') {
                    self.pos = saved_pos2;
                    self.last_was_term = true;
                    return Ok(Token::Ident(ident));
                }
                self.pos = saved_pos2;

                // Perl: `x` is the string-repetition infix operator only after a complete term.
                // After `sub`, `package`, `(`, etc. a term is expected — bare `x` must be an
                // identifier (`sub x {`, `x::Foo`, leading `x` in `(x)`).
                let tok = if ident == "x" && !self.last_was_term {
                    Token::Ident("x".to_string())
                } else {
                    keyword_or_ident(&ident)
                };
                // Keywords that expect a variable next should not set last_was_term
                // so that % is parsed as hash sigil, not modulo
                self.last_was_term = match ident.as_str() {
                    // Keywords/builtins that always expect arguments — never a term,
                    // so the next `/` is always a regex start.
                    "my"
                    | "mysync"
                    | "frozen"
                    | "const"
                    | "typed"
                    | "our"
                    | "local"
                    | "state"
                    | "return"
                    | "print"
                    | "pr"
                    | "say"
                    | "p"
                    | "die"
                    | "warn"
                    | "push"
                    | "pop"
                    | "shift"
                    | "shuffle"
                    | "chunked"
                    | "windowed"
                    | "unshift"
                    | "splice"
                    | "delete"
                    | "exists"
                    | "chomp"
                    | "chop"
                    | "defined"
                    | "keys"
                    | "values"
                    | "each"
                    | "sub"
                    | "struct"
                    | "if"
                    | "unless"
                    | "while"
                    | "until"
                    | "for"
                    | "foreach"
                    | "elsif"
                    | "use"
                    | "no"
                    | "require"
                    | "eval"
                    | "do"
                    | "map"
                    | "maps"
                    | "flat_maps"
                    | "grep"
                    | "greps"
                    | "sort"
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
                    | "with_index"
                    | "pmap"
                    | "pflat_map"
                    | "puniq"
                    | "pfirst"
                    | "pany"
                    | "pmap_chunked"
                    | "pipeline"
                    | "pgrep"
                    | "pfor"
                    | "par_lines"
                    | "par_walk"
                    | "pwatch"
                    | "watch"
                    | "psort"
                    | "reduce"
                    | "fold"
                    | "inject"
                    | "first"
                    | "detect"
                    | "find"
                    | "find_all"
                    | "preduce"
                    | "preduce_init"
                    | "pmap_reduce"
                    | "pcache"
                    | "fan"
                    | "fan_cap"
                    | "pchannel"
                    | "pselect"
                    | "uniq"
                    | "distinct"
                    | "flatten"
                    | "set"
                    | "list_count"
                    | "list_size"
                    | "count"
                    | "len"
                    | "size"
                    | "cnt"
                    | "zip"
                    | "async"
                    | "trace"
                    | "timer"
                    | "await"
                    | "slurp"
                    | "capture"
                    | "fetch_url"
                    | "fetch"
                    | "fetch_json"
                    | "fetch_async"
                    | "fetch_async_json"
                    | "par_fetch"
                    | "par_csv_read"
                    | "par_pipeline"
                    | "par_pipeline_stream"
                    | "par_sed"
                    | "join"
                    | "json_encode"
                    | "json_decode"
                    | "json_jq"
                    | "jwt_encode"
                    | "jwt_decode"
                    | "jwt_decode_unsafe"
                    | "log_info"
                    | "log_warn"
                    | "log_error"
                    | "log_debug"
                    | "log_trace"
                    | "log_json"
                    | "log_level"
                    | "sha256"
                    | "sha1"
                    | "md5"
                    | "hmac_sha256"
                    | "hmac"
                    | "uuid"
                    | "base64_encode"
                    | "base64_decode"
                    | "hex_encode"
                    | "hex_decode"
                    | "gzip"
                    | "gunzip"
                    | "zstd"
                    | "zstd_decode"
                    | "datetime_utc"
                    | "datetime_from_epoch"
                    | "datetime_parse_rfc3339"
                    | "datetime_strftime"
                    | "toml_decode"
                    | "toml_encode"
                    | "yaml_decode"
                    | "yaml_encode"
                    | "url_encode"
                    | "url_decode"
                    | "uri_escape"
                    | "uri_unescape"
                    | "split"
                    | "reverse"
                    | "reversed"
                    | "not"
                    | "ref"
                    | "scalar"
                    | "try"
                    | "catch"
                    | "finally"
                    | "given"
                    | "when"
                    | "default"
                    | "eval_timeout"
                    | "tie"
                    | "retry"
                    | "rate_limit"
                    | "every"
                    | "gen"
                    | "yield"
                    | "match"
                    | "filter"
                    | "f"
                    | "reject"
                    | "collect"
                    | "compact"
                    | "concat"
                    | "chain"
                    | "min_by"
                    | "max_by"
                    | "sort_by"
                    | "tally"
                    | "find_index"
                    | "each_with_index"
                    | "fore"
                    | "e"
                    | "ep"
                    | "flat_map"
                    | "group_by"
                    | "chunk_by"
                    | "bench" => false,
                    // `thread`/`t` are ambiguous: at statement start they're the
                    // thread keyword (expect args → false), but after an operator
                    // they could be variable names (e.g., `$x / t / 2` → true).
                    "thread" | "t" => !self.last_was_term,
                    _ => matches!(tok, Token::Ident(_)),
                };
                Ok(tok)
            }

            c => Err(self.syntax_err(format!("Unexpected character '{c}'"), self.line)),
        }
    }

    /// Tokenize entire input.
    pub fn tokenize(&mut self) -> PerlResult<Vec<(Token, usize)>> {
        let mut tokens = Vec::new();
        loop {
            // Skip whitespace/comments first so `self.line` reflects the
            // line where the upcoming token *starts*, not where the previous
            // token ended.  `next_token()` calls `skip_whitespace_and_comments`
            // again internally, but that second call is a harmless no-op.
            self.skip_whitespace_and_comments();
            let line = self.line;
            let tok = self.next_token()?;
            if tok == Token::Eof {
                tokens.push((Token::Eof, line));
                break;
            }
            tokens.push((tok, line));
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;

    #[test]
    fn tokenize_empty_yields_eof() {
        let mut l = Lexer::new("");
        let t = l.tokenize().expect("tokenize");
        assert_eq!(t.len(), 1);
        assert!(matches!(t[0].0, Token::Eof));
    }

    #[test]
    fn tokenize_integer_literal() {
        let mut l = Lexer::new("42");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(42)));
    }

    #[test]
    fn tokenize_keyword_my_and_semicolon() {
        let mut l = Lexer::new("my;");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "my"));
        assert!(matches!(t[1].0, Token::Semicolon));
    }

    #[test]
    fn tokenize_skips_hash_line_comment() {
        let mut l = Lexer::new("1#comment\n2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(1)));
        assert!(matches!(t[1].0, Token::Integer(2)));
        assert!(matches!(t[2].0, Token::Eof));
    }

    #[test]
    fn tokenize_double_quoted_string_literal() {
        let mut l = Lexer::new(r#""hi""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "hi"));
    }

    #[test]
    fn tokenize_double_string_escaped_sigils_are_literal() {
        // `\$` in source becomes a sentinel + parser emits literal `$` (not outer interpolation).
        let mut l = Lexer::new(r#""my \$x""#);
        let t = l.tokenize().expect("tokenize");
        let want = format!("my {}x", LITERAL_DOLLAR_IN_DQUOTE);
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if *s == want));
    }

    #[test]
    fn tokenize_double_string_braced_hex_unicode_escape() {
        let mut l = Lexer::new(r#""\x{1215}""#);
        let t = l.tokenize().expect("tokenize");
        let want: String = ['\u{1215}'].into_iter().collect();
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if *s == want));
    }

    #[test]
    fn tokenize_double_string_braced_unicode_u_escape() {
        let mut l = Lexer::new(r#""\u{0301}""#);
        let t = l.tokenize().expect("tokenize");
        let want: String = ['\u{0301}'].into_iter().collect();
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if *s == want));
    }

    #[test]
    fn tokenize_double_string_braced_unicode_u_escape_multi() {
        // \u{0041} = 'A', \u{00E9} = 'é', \u{1F600} = '😀'
        let mut l = Lexer::new(r#""\u{0041}\u{00E9}\u{1F600}""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "Aé😀"));
    }

    #[test]
    fn tokenize_double_string_octal_escape() {
        let mut l = Lexer::new(r#""\101""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "A"));
    }

    #[test]
    fn tokenize_double_string_braced_octal_escape() {
        let mut l = Lexer::new(r#""\o{101}""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "A"));
    }

    #[test]
    fn tokenize_double_string_control_char_escape() {
        let mut l = Lexer::new(r#""\cA""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "\x01"));
    }

    #[test]
    fn tokenize_double_string_named_unicode_escape() {
        let mut l = Lexer::new(r#""\N{SNOWMAN}""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "☃"));
    }

    #[test]
    fn tokenize_double_string_named_unicode_u_plus() {
        let mut l = Lexer::new(r#""\N{U+2603}""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "☃"));
    }

    #[test]
    fn tokenize_double_string_unbraced_hex_two_digits() {
        let mut l = Lexer::new(r#""\x41""#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "A"));
    }

    #[test]
    fn tokenize_single_quoted_string_literal() {
        let mut l = Lexer::new("'x'");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::SingleString(ref s) if s == "x"));
    }

    #[test]
    fn tokenize_spaceship_operator() {
        let mut l = Lexer::new("1 <=> 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(1)));
        assert!(matches!(t[1].0, Token::Spaceship));
        assert!(matches!(t[2].0, Token::Integer(2)));
    }

    #[test]
    fn tokenize_m_regex_literal() {
        let mut l = Lexer::new("m/abc/");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Regex(ref p, ref f, _) if p == "abc" && f.is_empty()));
    }

    #[test]
    fn tokenize_q_brace_constructor() {
        let mut l = Lexer::new("q{lit}");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::SingleString(ref s) if s == "lit"));
    }

    /// `q(sub ($) { 1 })` — nested `()` must not end at the `)` in `($)` (core `Carp.pm`).
    #[test]
    fn tokenize_q_paren_balances_nested_parens_in_prototype() {
        let mut l = Lexer::new("q(fn ($) { 1 })");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::SingleString(ref s) if s == "fn ($) { 1 }"));
    }

    /// `qw( (SV*)x )` — nested `()` inside `qw(...)` (core `B.pm`).
    #[test]
    fn tokenize_qw_paren_balances_nested_parens() {
        let mut l = Lexer::new("qw( (SV*)pWARN_ALL )");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::QW(ref w) if w.len() == 1 && w[0] == "(SV*)pWARN_ALL"));
    }

    #[test]
    fn tokenize_float_literal() {
        let mut l = Lexer::new("3.25");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Float(f) if (f - 3.25).abs() < f64::EPSILON));
    }

    #[test]
    fn tokenize_scientific_float() {
        let mut l = Lexer::new("1e2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Float(f) if (f - 100.0).abs() < 1e-9));
    }

    #[test]
    fn tokenize_hex_with_underscore_separators() {
        let mut l = Lexer::new("0x_FF");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(255)));
    }

    #[test]
    fn tokenize_qr_regex_with_flags() {
        let mut l = Lexer::new("qr/pat/i");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Regex(ref p, ref f, _) if p == "pat" && f == "i"));
    }

    #[test]
    fn tokenize_m_slash_includes_gc_flags() {
        let mut l = Lexer::new("m/./gc");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(&t[0].0, Token::Regex(p, f, _) if p == "." && f == "gc"));
    }

    #[test]
    fn tokenize_m_hash_delimiter_includes_gc_flags() {
        let mut l = Lexer::new("m#\\w#gc");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(&t[0].0, Token::Regex(p, f, _) if p == r"\w" && f == "gc"));
    }

    #[test]
    fn tokenize_qr_slash_includes_gco_flags() {
        let mut l = Lexer::new("qr/x/gco");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(&t[0].0, Token::Regex(p, f, _) if p == "x" && f == "gco"));
    }

    #[test]
    fn tokenize_qw_hash_delimiter_not_line_comment() {
        // `#` after `qw` must be the opener, not `skip_whitespace_and_comments` eating the line.
        let mut l = Lexer::new("qw# a b #;");
        let t = l.tokenize().expect("tokenize");
        assert!(
            matches!(&t[0].0, Token::QW(w) if w == &["a", "b"]),
            "first={:?}",
            t.first()
        );
    }

    #[test]
    fn tokenize_qq_hash_delimiter_single_line() {
        let mut l = Lexer::new("qq#x#;");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(&t[0].0, Token::DoubleString(s) if s == "x"));
    }

    #[test]
    fn tokenize_qr_hash_delimiter_text_balanced_preamble() {
        let src = "qr#(\n    [!=]~\n    | split|grep|map\n    | not|and|or|xor\n)#x";
        let mut l = Lexer::new(src);
        let t = l.tokenize().expect("tokenize");
        let Token::Regex(p, f, _) = &t[0].0 else {
            panic!("expected Regex, got {:?}", t[0].0);
        };
        let rest: Vec<_> = t.iter().skip(1).take(8).map(|x| &x.0).collect();
        assert!(f.contains('x'), "flags={f:?} pattern={p:?} rest={rest:?}");
        assert!(p.contains("[!=]~"), "{p:?}");
        assert!(p.contains("split|grep|map"), "{p:?}");
    }

    #[test]
    fn tokenize_octal_integer_literal() {
        let mut l = Lexer::new("010");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(8)));
    }

    #[test]
    fn tokenize_binary_integer_literal() {
        let mut l = Lexer::new("0b1010");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(10)));
    }

    #[test]
    fn tokenize_filetest_exists() {
        let mut l = Lexer::new("-e '.'");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::FileTest('e')));
        assert!(matches!(t[1].0, Token::SingleString(ref s) if s == "."));
    }

    #[test]
    fn tokenize_filetest_tty() {
        let mut l = Lexer::new("-t 'STDIN'");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::FileTest('t')));
        assert!(matches!(t[1].0, Token::SingleString(ref s) if s == "STDIN"));
    }

    #[test]
    fn tokenize_power_and_range_operators() {
        let mut l = Lexer::new("2 ** 3");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(2)));
        assert!(matches!(t[1].0, Token::Power));
        assert!(matches!(t[2].0, Token::Integer(3)));

        let mut l = Lexer::new("1..4");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(1)));
        assert!(matches!(t[1].0, Token::Range));
        assert!(matches!(t[2].0, Token::Integer(4)));
    }

    #[test]
    fn tokenize_numeric_equality_operators() {
        let mut l = Lexer::new("1 == 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(1)));
        assert!(matches!(t[1].0, Token::NumEq));
        assert!(matches!(t[2].0, Token::Integer(2)));

        let mut l = Lexer::new("3 != 4");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(3)));
        assert!(matches!(t[1].0, Token::NumNe));
        assert!(matches!(t[2].0, Token::Integer(4)));
    }

    #[test]
    fn tokenize_logical_and_or_plus_assign() {
        let mut l = Lexer::new("1 && 0");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(1)));
        assert!(matches!(t[1].0, Token::LogAnd));
        assert!(matches!(t[2].0, Token::Integer(0)));

        let mut l = Lexer::new("0 || 9");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(0)));
        assert!(matches!(t[1].0, Token::LogOr));
        assert!(matches!(t[2].0, Token::Integer(9)));

        let mut l = Lexer::new("n += 1");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "n"));
        assert!(matches!(t[1].0, Token::PlusAssign));
        assert!(matches!(t[2].0, Token::Integer(1)));
    }

    #[test]
    fn tokenize_bitwise_and_operator() {
        let mut l = Lexer::new("3 & 5");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(3)));
        assert!(matches!(t[1].0, Token::BitAnd));
        assert!(matches!(t[2].0, Token::Integer(5)));
    }

    #[test]
    fn tokenize_braced_caret_scalar_global_phase() {
        let mut l = Lexer::new(r#"print ${^GLOBAL_PHASE}, "\n";"#);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "print"));
        assert!(matches!(t[1].0, Token::ScalarVar(ref s) if s == "^GLOBAL_PHASE"));
        assert!(matches!(t[2].0, Token::Comma));
        assert!(matches!(t[3].0, Token::DoubleString(ref s) if s == "\n"));
        assert!(matches!(t[4].0, Token::Semicolon));
    }

    #[test]
    fn tokenize_bitwise_or_and_assign() {
        let mut l = Lexer::new("$a |= $b");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "a"));
        assert!(matches!(t[1].0, Token::BitOrAssign));
        assert!(matches!(t[2].0, Token::ScalarVar(ref s) if s == "b"));

        let mut l = Lexer::new("$a &= $b");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::BitAndAssign));
    }

    #[test]
    fn tokenize_division_and_modulo() {
        let mut l = Lexer::new("7 / 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::Slash));

        let mut l = Lexer::new("7 % 3");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::Percent));
    }

    #[test]
    fn tokenize_comma_fat_arrow_and_semicolon() {
        let mut l = Lexer::new("a => 1;");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "a"));
        assert!(matches!(t[1].0, Token::FatArrow));
        assert!(matches!(t[2].0, Token::Integer(1)));
        assert!(matches!(t[3].0, Token::Semicolon));
    }

    #[test]
    fn tokenize_minus_unary_vs_binary() {
        let mut l = Lexer::new("- 5");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Minus));
        assert!(matches!(t[1].0, Token::Integer(5)));
    }

    #[test]
    fn tokenize_dollar_scalar_sigil() {
        let mut l = Lexer::new("$foo");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "foo"));
    }

    /// `=` + letter is assignment unless `=` starts the line (POD). `$_=foo` must not skip POD.
    #[test]
    fn tokenize_assign_not_pod_when_eq_not_line_start() {
        let mut l = Lexer::new("$_=foo;");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "_"));
        assert!(matches!(t[1].0, Token::Assign));
        assert!(matches!(t[2].0, Token::Ident(ref s) if s == "foo"));
        assert!(matches!(t[3].0, Token::Semicolon));
    }

    #[test]
    fn tokenize_pod_equals_still_skipped_at_line_start() {
        let mut l = Lexer::new("=head1 NAME\ncode\n=cut\n$x;");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "x"));
        assert!(matches!(t[1].0, Token::Semicolon));
    }

    #[test]
    fn tokenize_at_array_sigil() {
        let mut l = Lexer::new("@arr");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ArrayVar(ref s) if s == "arr"));
    }

    #[test]
    fn tokenize_at_caret_capture_array() {
        let mut l = Lexer::new("@^CAPTURE");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ArrayVar(ref s) if s == "^CAPTURE"));
    }

    #[test]
    fn tokenize_percent_caret_hook_hash() {
        let mut l = Lexer::new("%^HOOK");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::HashVar(ref s) if s == "^HOOK"));
    }

    #[test]
    fn tokenize_caret_letter_and_at_minus_plus() {
        let mut l = Lexer::new("$^I@-@+");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "^I"));
        assert!(matches!(t[1].0, Token::ArrayVar(ref s) if s == "-"));
        assert!(matches!(t[2].0, Token::ArrayVar(ref s) if s == "+"));
    }

    #[test]
    fn tokenize_percent_hash_sigil() {
        let mut l = Lexer::new("%h");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::HashVar(ref s) if s == "h"));
    }

    #[test]
    fn tokenize_percent_plus_named_capture_hash() {
        let mut l = Lexer::new("%+");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::HashVar(ref s) if s == "+"));
    }

    #[test]
    fn tokenize_dollar_dollar_under_brace_is_not_pid() {
        // `$$_{$k}` — second `$$` is not PID; tokenizes as `$_` then `{` (Perl `$_->{$k}`).
        let mut l = Lexer::new("$$_{$k}");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "_"));
        assert!(matches!(t[1].0, Token::LBrace));
    }

    #[test]
    fn tokenize_braced_scalar_deref_try_tiny() {
        // `${$code_ref}` ≡ `$$code_ref` (Try::Tiny blesses scalar refs to coderefs).
        let mut l = Lexer::new("${$code_ref}");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DerefScalarVar(ref s) if s == "code_ref"));
    }

    #[test]
    fn tokenize_braced_scalar_deref_package_qualified() {
        let mut l = Lexer::new("${$Foo::bar}");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DerefScalarVar(ref s) if s == "Foo::bar"));
    }

    #[test]
    fn tokenize_dollar_colon_stash_brace() {
        // `$::{$k}` — `%::` main stash (core Carp.pm line 32).
        let mut l = Lexer::new("$::{$pack}");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ScalarVar(ref s) if s == "::"));
        assert!(matches!(t[1].0, Token::LBrace));
    }

    #[test]
    fn tokenize_ampersand_then_ident_is_bitand_not_coderef() {
        // Subroutine coderef `&name` is not a distinct token; lexer emits `&` then ident.
        let mut l = Lexer::new("&f");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::BitAnd));
        assert!(matches!(t[1].0, Token::Ident(ref s) if s == "f"));
    }

    #[test]
    fn tokenize_qq_paren_constructor() {
        let mut l = Lexer::new("qq(x y)");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if s == "x y"));
    }

    #[test]
    fn tokenize_qq_slash_escaped_dollar_is_literal() {
        let mut l = Lexer::new(r#"qq/my \$y/"#);
        let t = l.tokenize().expect("tokenize");
        let want = format!("my {}y", LITERAL_DOLLAR_IN_DQUOTE);
        assert!(matches!(t[0].0, Token::DoubleString(ref s) if *s == want));
    }

    #[test]
    fn tokenize_s_substitution_alternate_delimiter() {
        let mut l = Lexer::new("s#a#b#");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s.starts_with("\x00s\x00")));
    }

    #[test]
    fn tokenize_tr_slash_delimiter() {
        let mut l = Lexer::new("tr/a/b/");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s.starts_with("\x00tr\x00")));
    }

    #[test]
    fn tokenize_y_synonym_for_tr() {
        let mut l = Lexer::new("y/x/y/");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s.starts_with("\x00tr\x00")));
    }

    #[test]
    fn tokenize_less_equal_greater_relops() {
        let mut l = Lexer::new("1 <= 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::NumLe));

        let mut l = Lexer::new("3 >= 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::NumGe));

        let mut l = Lexer::new("1 < 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::NumLt));

        let mut l = Lexer::new("3 > 2");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::NumGt));
    }

    #[test]
    fn tokenize_readline_scalar_handle() {
        let mut l = Lexer::new("<$fh>");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::ReadLine(ref s) if s == "fh"));
    }

    #[test]
    fn tokenize_shift_right_and_shift_left_assign() {
        let mut l = Lexer::new("8 >> 1");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::ShiftRight));

        let mut l = Lexer::new("8 << 1");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::ShiftLeft));

        let mut l = Lexer::new("x <<= 3");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::ShiftLeftAssign));
    }

    #[test]
    fn tokenize_heredoc_after_print_not_shift() {
        let src = "print <<EOT\nhi\nEOT\n";
        let mut l = Lexer::new(src);
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "print"));
        assert!(
            matches!(&t[1].0, Token::HereDoc(tag, body, interpolate) if tag == "EOT" && body == "hi\n" && *interpolate),
            "got {:?}",
            t[1].0
        );
    }

    #[test]
    fn tokenize_bitwise_or_xor() {
        let mut l = Lexer::new("3 | 1");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::BitOr));

        let mut l = Lexer::new("3 ^ 1");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::BitXor));
    }

    #[test]
    fn tokenize_pipe_forward_vs_bitor_vs_logor() {
        // `|>` must lex as a distinct token (not `|` followed by `>`).
        let mut l = Lexer::new("1 |> f");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::PipeForward), "got {:?}", t[1].0);

        // Make sure `|` and `||` still work alongside `|>`.
        let mut l = Lexer::new("a | b || c |> d");
        let t = l.tokenize().expect("tokenize");
        let kinds: Vec<_> = t.iter().map(|(k, _)| k.clone()).collect();
        assert!(kinds.iter().any(|k| matches!(k, Token::BitOr)));
        assert!(kinds.iter().any(|k| matches!(k, Token::LogOr)));
        assert!(kinds.iter().any(|k| matches!(k, Token::PipeForward)));
    }

    #[test]
    fn tokenize_compare_and_three_way_string_ops() {
        let mut l = Lexer::new("\"a\" cmp \"b\"");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::StrCmp));
    }

    #[test]
    fn tokenize_package_double_colon_splits_qualified_name() {
        let mut l = Lexer::new("Foo::Bar::baz");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "Foo"));
        assert!(matches!(t[1].0, Token::PackageSep));
        assert!(matches!(t[2].0, Token::Ident(ref s) if s == "Bar"));
        assert!(matches!(t[3].0, Token::PackageSep));
        assert!(matches!(t[4].0, Token::Ident(ref s) if s == "baz"));
    }

    #[test]
    fn tokenize_pod_line_skipped_like_comment_prefix() {
        // `=head1` at line start starts POD; lexer should skip until =cut
        let mut l = Lexer::new("=pod\n=cut\n42");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Integer(42)));
    }

    #[test]
    fn tokenize_underscore_in_identifier() {
        let mut l = Lexer::new("__PACKAGE__");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "__PACKAGE__"));
    }

    /// `x` is the repetition operator only in infix position; after `sub` it is a sub name (Perl).
    #[test]
    fn tokenize_x_repeat_vs_sub_name() {
        let mut l = Lexer::new("3 x 4");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::X));

        let mut l = Lexer::new("sub x { 1 }");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::Ident(ref s) if s == "sub"));
        assert!(matches!(t[1].0, Token::Ident(ref s) if s == "x"));
    }
}
