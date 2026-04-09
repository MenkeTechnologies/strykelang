use crate::error::{PerlError, PerlResult};
use crate::token::{keyword_or_ident, Token};

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    pub line: usize,
    /// Tracks whether the last token was a term (value/variable/close-delim)
    /// to disambiguate `/` as division vs regex and `{` as hash-ref vs block.
    last_was_term: bool,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            last_was_term: false,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.input.get(self.pos + offset).copied()
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
                .map_err(|_| PerlError::syntax("Invalid hex literal", self.line))?;
            return Ok(Token::Integer(val));
        }
        if is_bin {
            let digits = self.read_while(|c| c == '0' || c == '1' || c == '_');
            let clean: String = digits.chars().filter(|&c| c != '_').collect();
            let val = i64::from_str_radix(&clean, 2)
                .map_err(|_| PerlError::syntax("Invalid binary literal", self.line))?;
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
                .map_err(|_| PerlError::syntax("Invalid float literal", self.line))?;
            Ok(Token::Float(val))
        } else if is_oct && clean.starts_with('0') && clean.len() > 1 {
            let val = i64::from_str_radix(&clean[1..], 8)
                .map_err(|_| PerlError::syntax("Invalid octal literal", self.line))?;
            Ok(Token::Integer(val))
        } else {
            let val: i64 = clean
                .parse()
                .map_err(|_| PerlError::syntax("Invalid integer literal", self.line))?;
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
                None => {
                    return Err(PerlError::syntax(
                        "Unterminated single-quoted string",
                        self.line,
                    ))
                }
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
                    Some('0') => s.push('\0'),
                    Some('a') => s.push('\x07'),
                    Some('b') => s.push('\x08'),
                    Some('f') => s.push('\x0C'),
                    Some('e') => s.push('\x1B'),
                    Some('x') => {
                        let hex = self.read_while(|c| c.is_ascii_hexdigit());
                        if let Ok(val) = u32::from_str_radix(&hex, 16) {
                            if let Some(c) = char::from_u32(val) {
                                s.push(c);
                            }
                        }
                    }
                    Some(c) if c == term => s.push(c),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => return Err(PerlError::syntax("Unterminated string", self.line)),
                },
                Some(c) if c == term => break,
                Some(c) => s.push(c),
                None => return Err(PerlError::syntax("Unterminated string", self.line)),
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
                None => return Err(PerlError::syntax("Unterminated regex", self.line)),
            }
        }
        let flags = self.read_while(|c| "gimsxe".contains(c));
        Ok(Token::Regex(pattern, flags))
    }

    fn read_qw(&mut self) -> PerlResult<Token> {
        // Already consumed 'qw', now expect delimiter
        self.skip_whitespace_and_comments();
        let open = self
            .advance()
            .ok_or_else(|| PerlError::syntax("Expected delimiter after qw", self.line))?;
        let close = match open {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '<' => '>',
            c => c,
        };
        let mut words = Vec::new();
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
                return Err(PerlError::syntax("Unterminated qw()", self.line));
            }
            let word = self.read_while(|c| !c.is_whitespace() && c != close);
            if !word.is_empty() {
                words.push(word);
            }
        }
        Ok(Token::QW(words))
    }

    fn read_heredoc_tag(&mut self) -> PerlResult<(String, bool)> {
        // We've consumed '<<'. Now figure out the tag.
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
                return self.read_heredoc_tag(); // recurse for the actual tag
            }
            _ => {
                tag = self.read_while(|c| c.is_alphanumeric() || c == '_');
                quoted = true;
            }
        }
        Ok((tag, quoted))
    }

    fn read_heredoc_body(&mut self, tag: &str) -> PerlResult<String> {
        // Read until we find a line that is exactly the tag
        let mut body = String::new();
        // First, skip to end of current line
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                self.advance();
                break;
            }
            self.advance();
        }
        loop {
            let _line_start = self.pos;
            let line = self.read_while(|c| c != '\n');
            if line.trim() == tag {
                break;
            }
            body.push_str(&line);
            body.push('\n');
            if self.peek() == Some('\n') {
                self.advance();
            } else if self.pos >= self.input.len() {
                return Err(PerlError::syntax(
                    format!("Unterminated heredoc (looking for '{tag}')"),
                    self.line,
                ));
            }
        }
        if self.peek() == Some('\n') {
            self.advance();
        }
        Ok(body)
    }

    fn read_identifier(&mut self) -> String {
        self.read_while(|c| c.is_alphanumeric() || c == '_')
    }

    fn read_variable_name(&mut self) -> String {
        // Handle special vars like $_, $!, $0, $/, etc.
        match self.peek() {
            Some(c) if c.is_alphabetic() || c == '_' => self.read_identifier(),
            Some(c) if "!@$&*+;',\"\\|?/<>.0123456789^~%-#=()[]{}".contains(c) => {
                self.advance();
                c.to_string()
            }
            Some('{') => {
                // ${name}
                self.advance(); // {
                let name = self.read_while(|c| c != '}');
                if self.peek() == Some('}') {
                    self.advance();
                }
                name
            }
            _ => String::new(),
        }
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
                let name = self.read_variable_name();
                if name.is_empty() {
                    return Err(PerlError::syntax(
                        "Expected variable name after $",
                        self.line,
                    ));
                }
                self.last_was_term = true;
                Ok(Token::ScalarVar(name))
            }
            '@' => {
                self.advance();
                if self.peek() == Some('_') || self.peek().is_some_and(|c| c.is_alphabetic()) {
                    let name = self.read_identifier();
                    self.last_was_term = true;
                    return Ok(Token::ArrayVar(name));
                }
                self.last_was_term = false;
                Ok(Token::ArrayAt)
            }
            '%' if !self.last_was_term => {
                self.advance();
                if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
                    let name = self.read_identifier();
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

            // Backtick
            '`' => {
                self.advance();
                let cmd = self.read_escaped_until('`')?;
                self.last_was_term = true;
                Ok(Token::DoubleString(cmd)) // treated as interpolated command
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
                        if "efdlpSszrwxoRWXOBCTMAgu".contains(c)
                            && self.peek_at(1).is_none_or(|n| {
                                n.is_whitespace() || n == '$' || n == '\'' || n == '"' || n == '('
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
                // POD: =head1 etc — skip until =cut
                if self.peek().is_some_and(|c| c.is_alphabetic()) {
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
                    // Heredoc
                    let (tag, _interpolate) = self.read_heredoc_tag()?;
                    let body = self.read_heredoc_body(&tag)?;
                    self.last_was_term = true;
                    return Ok(Token::HereDoc(tag, body));
                }
                self.last_was_term = false;
                Ok(Token::NumLt)
            }
            '>' => {
                self.advance();
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
                    "qw" => {
                        let tok = self.read_qw()?;
                        self.last_was_term = true;
                        return Ok(tok);
                    }
                    "qq" | "q" => {
                        self.skip_whitespace_and_comments();
                        let delim = self.advance().ok_or_else(|| {
                            PerlError::syntax("Expected delimiter after q/qq", self.line)
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
                        if ident == "qq" {
                            return Ok(Token::DoubleString(s));
                        }
                        return Ok(Token::SingleString(s));
                    }
                    "qr" => {
                        self.skip_whitespace_and_comments();
                        let delim = self.advance().ok_or_else(|| {
                            PerlError::syntax("Expected delimiter after qr", self.line)
                        })?;
                        let close = match delim {
                            '(' => ')',
                            '[' => ']',
                            '{' => '}',
                            '<' => '>',
                            c => c,
                        };
                        let pattern = self.read_escaped_until(close)?;
                        let flags = self.read_while(|c| "gimsxe".contains(c));
                        self.last_was_term = true;
                        return Ok(Token::Regex(pattern, flags));
                    }
                    "m" => {
                        // m/pattern/flags
                        if let Some(delim) = self.peek() {
                            if !delim.is_alphanumeric() && delim != '_' {
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
                                            return Err(PerlError::syntax(
                                                "Unterminated m// pattern",
                                                self.line,
                                            ))
                                        }
                                    }
                                }
                                let flags = self.read_while(|c| "gimsxe".contains(c));
                                self.last_was_term = true;
                                return Ok(Token::Regex(pattern, flags));
                            }
                        }
                        // Just the identifier 'm'
                        self.last_was_term = true;
                        return Ok(Token::Ident(ident));
                    }
                    "s" => {
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
                                            return Err(PerlError::syntax(
                                                "Unterminated s/// pattern",
                                                self.line,
                                            ))
                                        }
                                    }
                                }
                                // For paired delimiters, read the opening of the replacement part
                                if "([{<".contains(delim) {
                                    self.skip_whitespace_and_comments();
                                    let open2 = self.advance().unwrap_or(delim);
                                    let close = match open2 {
                                        '(' => ')',
                                        '[' => ']',
                                        '{' => '}',
                                        '<' => '>',
                                        c => c,
                                    };
                                    let replacement = self.read_escaped_until(close)?;
                                    let flags = self.read_while(|c| "gimsxe".contains(c));
                                    self.last_was_term = true;
                                    // Encode as special token — parser will decode
                                    return Ok(Token::Ident(format!(
                                        "\x00s\x00{}\x00{}\x00{}",
                                        pattern, replacement, flags
                                    )));
                                }
                                let replacement = self.read_escaped_until(close)?;
                                let flags = self.read_while(|c| "gimsxe".contains(c));
                                self.last_was_term = true;
                                return Ok(Token::Ident(format!(
                                    "\x00s\x00{}\x00{}\x00{}",
                                    pattern, replacement, flags
                                )));
                            }
                        }
                        self.last_was_term = true;
                        return Ok(Token::Ident(ident));
                    }
                    "tr" | "y" => {
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
                                    self.skip_whitespace_and_comments();
                                    self.advance(); // open second pair
                                }
                                let to = self.read_escaped_until(close)?;
                                let flags = self.read_while(|c| "cdsr".contains(c));
                                self.last_was_term = true;
                                return Ok(Token::Ident(format!(
                                    "\x00tr\x00{}\x00{}\x00{}",
                                    from, to, flags
                                )));
                            }
                        }
                        self.last_was_term = true;
                        return Ok(Token::Ident(ident));
                    }
                    _ => {}
                }

                // Check for label: IDENT followed by ':'
                // But not 'eq:', 'ne:', etc. — those are operators
                let saved_pos = self.pos;
                self.skip_whitespace_and_comments();
                if self.peek() == Some(':') && self.peek_at(1) != Some(':') {
                    // Could be a label or a ternary else
                    // Labels are uppercase by convention but not required
                    // We'll treat ALLCAPS: as labels
                    if ident.chars().all(|c| c.is_uppercase() || c == '_') {
                        self.advance(); // consume ':'
                        self.last_was_term = false;
                        return Ok(Token::Label(ident));
                    }
                }
                self.pos = saved_pos;

                // Fat arrow lookahead: ident followed by => is a string
                let saved_pos2 = self.pos;
                self.skip_whitespace_and_comments();
                if self.peek() == Some('=') && self.peek_at(1) == Some('>') {
                    self.pos = saved_pos2;
                    self.last_was_term = true;
                    return Ok(Token::Ident(ident));
                }
                self.pos = saved_pos2;

                let tok = keyword_or_ident(&ident);
                // Keywords that expect a variable next should not set last_was_term
                // so that % is parsed as hash sigil, not modulo
                self.last_was_term = match ident.as_str() {
                    "my" | "our" | "local" | "return" | "print" | "say" | "die" | "warn"
                    | "push" | "pop" | "shift" | "unshift" | "splice" | "delete" | "exists"
                    | "chomp" | "chop" | "defined" | "keys" | "values" | "each" | "sub" | "if"
                    | "unless" | "while" | "until" | "for" | "foreach" | "elsif" | "use" | "no"
                    | "require" | "eval" | "do" | "map" | "grep" | "sort" | "pmap" | "pgrep"
                    | "pfor" | "psort" | "join" | "split" | "reverse" | "not" | "ref"
                    | "scalar" => false,
                    _ => matches!(tok, Token::Ident(_)),
                };
                Ok(tok)
            }

            c => Err(PerlError::syntax(
                format!("Unexpected character '{c}'"),
                self.line,
            )),
        }
    }

    /// Tokenize entire input.
    pub fn tokenize(&mut self) -> PerlResult<Vec<(Token, usize)>> {
        let mut tokens = Vec::new();
        loop {
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
