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
                return Err(PerlError::syntax(
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
            Some(c) if c.is_alphabetic() || c == '_' => self.read_package_qualified_identifier(),
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
            Some(c) if "!@$&*+;',\"\\|?/<>.0123456789~%-#=()[]{}".contains(c) => {
                self.advance();
                c.to_string()
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
                // `$$foo` — symbolic scalar deref (Perl `${$foo}`-style lookup)
                if self.peek() == Some('$') {
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
                        if "efdlpSszrwxoRWXOBCTMAgut".contains(c)
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
                    "format" => {
                        self.skip_whitespace_and_comments();
                        let fname = self.read_package_qualified_identifier();
                        self.skip_whitespace_and_comments();
                        if self.peek() != Some('=') {
                            return Err(PerlError::syntax(
                                "Expected '=' after format name",
                                self.line,
                            ));
                        }
                        self.advance();
                        let lines = self.read_format_body()?;
                        self.last_was_term = false;
                        return Ok(Token::FormatDecl { name: fname, lines });
                    }
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
                    "my"
                    | "mysync"
                    | "frozen"
                    | "typed"
                    | "our"
                    | "local"
                    | "return"
                    | "print"
                    | "say"
                    | "die"
                    | "warn"
                    | "push"
                    | "pop"
                    | "shift"
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
                    | "grep"
                    | "sort"
                    | "pmap"
                    | "pmap_chunked"
                    | "pipeline"
                    | "pgrep"
                    | "pfor"
                    | "par_lines"
                    | "pwatch"
                    | "watch"
                    | "psort"
                    | "reduce"
                    | "preduce"
                    | "preduce_init"
                    | "pmap_reduce"
                    | "pcache"
                    | "fan"
                    | "fan_cap"
                    | "pchannel"
                    | "pselect"
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
                    | "join"
                    | "json_encode"
                    | "json_decode"
                    | "split"
                    | "reverse"
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
                    | "match" => false,
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
        assert!(matches!(t[0].0, Token::Regex(ref p, ref f) if p == "abc" && f.is_empty()));
    }

    #[test]
    fn tokenize_q_brace_constructor() {
        let mut l = Lexer::new("q{lit}");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[0].0, Token::SingleString(ref s) if s == "lit"));
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
        assert!(matches!(t[0].0, Token::Regex(ref p, ref f) if p == "pat" && f == "i"));
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
    fn tokenize_shift_right_and_shift_left_assign() {
        // Bare `<<` starts a heredoc in this lexer; `>>` is bitwise shift right.
        let mut l = Lexer::new("8 >> 1");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::ShiftRight));

        let mut l = Lexer::new("x <<= 3");
        let t = l.tokenize().expect("tokenize");
        assert!(matches!(t[1].0, Token::ShiftLeftAssign));
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
}
