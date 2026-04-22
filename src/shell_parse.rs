//! Zsh/Bash compatible shell parser for zshrs
//!
//! Parses POSIX sh, bash, and zsh syntax into an AST that can be executed.

use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum ShellToken {
    Word(String),
    Number(i64),

    // Operators
    Semi,           // ;
    Newline,        // \n
    Amp,            // &
    AmpAmp,         // &&
    Pipe,           // |
    PipePipe,       // ||
    LParen,         // (
    RParen,         // )
    LBrace,         // {
    RBrace,         // }
    LBracket,       // [
    RBracket,       // ]
    DoubleLBracket, // [[
    DoubleRBracket, // ]]

    // Redirections
    Less,                    // <
    Greater,                 // >
    GreaterGreater,          // >>
    LessGreater,             // <>
    GreaterAmp,              // >&
    LessAmp,                 // <&
    GreaterPipe,             // >|
    LessLess,                // <<
    LessLessLess,            // <<<
    HereDoc(String, String), // << with (delimiter, content)
    AmpGreater,              // &>  (zsh: redirect both stdout and stderr)
    AmpGreaterGreater,       // &>> (zsh: append both stdout and stderr)

    // Arithmetic
    DoubleLParen, // ((
    DoubleRParen, // ))

    // Keywords
    If,
    Then,
    Else,
    Elif,
    Fi,
    Case,
    Esac,
    For,
    While,
    Until,
    Do,
    Done,
    In,
    Function,
    Select,
    Time,
    Coproc,

    // Special
    Bang,        // !
    DoubleSemi,  // ;;
    SemiAmp,     // ;&
    SemiSemiAmp, // ;;&

    Eof,
}

#[derive(Debug, Clone)]
pub enum ShellWord {
    Literal(String),
    SingleQuoted(String),
    DoubleQuoted(Vec<ShellWord>),
    Variable(String),
    VariableBraced(String, Option<Box<VarModifier>>),
    ArrayVar(String, Box<ShellWord>), // ${arr[idx]}
    CommandSub(Box<ShellCommand>),
    ProcessSubIn(Box<ShellCommand>), // <(cmd) - read from command output
    ProcessSubOut(Box<ShellCommand>), // >(cmd) - write to command input
    ArithSub(String),
    ArrayLiteral(Vec<ShellWord>), // (a b c)
    Glob(String),
    Tilde(Option<String>),
    Concat(Vec<ShellWord>),
}

#[derive(Debug, Clone)]
pub enum VarModifier {
    Default(ShellWord),               // ${var:-word}
    DefaultAssign(ShellWord),         // ${var:=word}
    Error(ShellWord),                 // ${var:?word}
    Alternate(ShellWord),             // ${var:+word}
    Length,                           // ${#var}
    ArrayLength,                      // ${#arr[@]} or ${#arr[*]}
    ArrayIndex(String),               // ${arr[idx]}
    ArrayAll,                         // ${arr[@]} or ${arr[*]}
    Substring(i64, Option<i64>),      // ${var:offset:length}
    RemovePrefix(ShellWord),          // ${var#pattern}
    RemovePrefixLong(ShellWord),      // ${var##pattern}
    RemoveSuffix(ShellWord),          // ${var%pattern}
    RemoveSuffixLong(ShellWord),      // ${var%%pattern}
    Replace(ShellWord, ShellWord),    // ${var/pat/repl}
    ReplaceAll(ShellWord, ShellWord), // ${var//pat/repl}
    Upper,                            // ${var^} or ${var^^}
    Lower,                            // ${var,} or ${var,,}
}

#[derive(Debug, Clone)]
pub enum ShellCommand {
    Simple(SimpleCommand),
    Pipeline(Vec<ShellCommand>, bool), // commands, negated
    List(Vec<(ShellCommand, ListOp)>),
    Compound(CompoundCommand),
    FunctionDef(String, Box<ShellCommand>),
}

#[derive(Debug, Clone)]
pub enum ListOp {
    And,  // &&
    Or,   // ||
    Semi, // ;
    Amp,  // &
    Newline,
}

#[derive(Debug, Clone)]
pub struct SimpleCommand {
    pub assignments: Vec<(String, ShellWord)>,
    pub words: Vec<ShellWord>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub fd: Option<i32>,
    pub op: RedirectOp,
    pub target: ShellWord,
    pub heredoc_content: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum RedirectOp {
    Write,      // >
    Append,     // >>
    Read,       // <
    ReadWrite,  // <>
    Clobber,    // >|
    DupRead,    // <&
    DupWrite,   // >&
    HereDoc,    // <<
    HereString, // <<<
    WriteBoth,  // &>  (zsh: stdout+stderr)
    AppendBoth, // &>> (zsh: append stdout+stderr)
}

#[derive(Debug, Clone)]
pub enum CompoundCommand {
    BraceGroup(Vec<ShellCommand>),
    Subshell(Vec<ShellCommand>),
    If {
        conditions: Vec<(Vec<ShellCommand>, Vec<ShellCommand>)>, // (condition, body) pairs
        else_part: Option<Vec<ShellCommand>>,
    },
    For {
        var: String,
        words: Option<Vec<ShellWord>>,
        body: Vec<ShellCommand>,
    },
    ForArith {
        init: String,
        cond: String,
        step: String,
        body: Vec<ShellCommand>,
    },
    While {
        condition: Vec<ShellCommand>,
        body: Vec<ShellCommand>,
    },
    Until {
        condition: Vec<ShellCommand>,
        body: Vec<ShellCommand>,
    },
    Case {
        word: ShellWord,
        cases: Vec<(Vec<ShellWord>, Vec<ShellCommand>, CaseTerminator)>,
    },
    Select {
        var: String,
        words: Option<Vec<ShellWord>>,
        body: Vec<ShellCommand>,
    },
    Coproc {
        name: Option<String>,
        body: Box<ShellCommand>,
    },
    Cond(CondExpr),
    Arith(String),
}

#[derive(Debug, Clone, Copy)]
pub enum CaseTerminator {
    Break,       // ;;
    Fallthrough, // ;&
    Continue,    // ;;&
}

#[derive(Debug, Clone)]
pub enum CondExpr {
    // File tests
    FileExists(ShellWord),     // -e
    FileRegular(ShellWord),    // -f
    FileDirectory(ShellWord),  // -d
    FileSymlink(ShellWord),    // -L/-h
    FileReadable(ShellWord),   // -r
    FileWritable(ShellWord),   // -w
    FileExecutable(ShellWord), // -x
    FileNonEmpty(ShellWord),   // -s

    // String tests
    StringEmpty(ShellWord),               // -z
    StringNonEmpty(ShellWord),            // -n
    StringEqual(ShellWord, ShellWord),    // = or ==
    StringNotEqual(ShellWord, ShellWord), // !=
    StringMatch(ShellWord, ShellWord),    // =~
    StringLess(ShellWord, ShellWord),     // <
    StringGreater(ShellWord, ShellWord),  // >

    // Numeric tests
    NumEqual(ShellWord, ShellWord),        // -eq
    NumNotEqual(ShellWord, ShellWord),     // -ne
    NumLess(ShellWord, ShellWord),         // -lt
    NumLessEqual(ShellWord, ShellWord),    // -le
    NumGreater(ShellWord, ShellWord),      // -gt
    NumGreaterEqual(ShellWord, ShellWord), // -ge

    // Logic
    Not(Box<CondExpr>),
    And(Box<CondExpr>, Box<CondExpr>),
    Or(Box<CondExpr>, Box<CondExpr>),
}

pub struct ShellLexer<'a> {
    input: Peekable<Chars<'a>>,
    line: usize,
    col: usize,
    at_line_start: bool,
}

impl<'a> ShellLexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
            line: 1,
            col: 1,
            at_line_start: true,
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.input.peek().copied()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.input.next();
        if let Some(ch) = c {
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
                self.at_line_start = true;
            } else {
                self.col += 1;
                self.at_line_start = false;
            }
        }
        c
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.next_char();
            } else if c == '\\' {
                // Line continuation
                self.next_char();
                if self.peek() == Some('\n') {
                    self.next_char();
                }
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while let Some(c) = self.peek() {
                if c == '\n' {
                    break;
                }
                self.next_char();
            }
        }
    }

    pub fn next_token(&mut self) -> ShellToken {
        self.skip_whitespace();
        self.skip_comment();

        let c = match self.peek() {
            Some(c) => c,
            None => return ShellToken::Eof,
        };

        // Newline
        if c == '\n' {
            self.next_char();
            return ShellToken::Newline;
        }

        // Multi-char operators first
        if c == ';' {
            self.next_char();
            if self.peek() == Some(';') {
                self.next_char();
                if self.peek() == Some('&') {
                    self.next_char();
                    return ShellToken::SemiSemiAmp;
                }
                return ShellToken::DoubleSemi;
            }
            if self.peek() == Some('&') {
                self.next_char();
                return ShellToken::SemiAmp;
            }
            return ShellToken::Semi;
        }

        if c == '&' {
            self.next_char();
            match self.peek() {
                Some('&') => {
                    self.next_char();
                    return ShellToken::AmpAmp;
                }
                Some('>') => {
                    self.next_char();
                    if self.peek() == Some('>') {
                        self.next_char();
                        return ShellToken::AmpGreaterGreater;
                    }
                    return ShellToken::AmpGreater;
                }
                _ => return ShellToken::Amp,
            }
        }

        if c == '|' {
            self.next_char();
            if self.peek() == Some('|') {
                self.next_char();
                return ShellToken::PipePipe;
            }
            return ShellToken::Pipe;
        }

        if c == '<' {
            self.next_char();
            match self.peek() {
                Some('(') => {
                    // Process substitution <(cmd)
                    self.next_char();
                    let cmd = self.read_process_sub();
                    return ShellToken::Word(format!("<({}", cmd));
                }
                Some('<') => {
                    self.next_char();
                    if self.peek() == Some('<') {
                        self.next_char();
                        return ShellToken::LessLessLess;
                    }
                    // << heredoc - read delimiter and content
                    return self.read_heredoc();
                }
                Some('>') => {
                    self.next_char();
                    return ShellToken::LessGreater;
                }
                Some('&') => {
                    self.next_char();
                    return ShellToken::LessAmp;
                }
                _ => return ShellToken::Less,
            }
        }

        if c == '>' {
            self.next_char();
            match self.peek() {
                Some('(') => {
                    // Process substitution >(cmd)
                    self.next_char();
                    let cmd = self.read_process_sub();
                    return ShellToken::Word(format!(">({}", cmd));
                }
                Some('>') => {
                    self.next_char();
                    return ShellToken::GreaterGreater;
                }
                Some('&') => {
                    self.next_char();
                    return ShellToken::GreaterAmp;
                }
                Some('|') => {
                    self.next_char();
                    return ShellToken::GreaterPipe;
                }
                _ => return ShellToken::Greater,
            }
        }

        if c == '(' {
            self.next_char();
            if self.peek() == Some('(') {
                self.next_char();
                return ShellToken::DoubleLParen;
            }
            return ShellToken::LParen;
        }

        if c == ')' {
            self.next_char();
            if self.peek() == Some(')') {
                self.next_char();
                return ShellToken::DoubleRParen;
            }
            return ShellToken::RParen;
        }

        if c == '[' {
            self.next_char();
            if self.peek() == Some('[') {
                self.next_char();
                return ShellToken::DoubleLBracket;
            }
            return ShellToken::LBracket;
        }

        if c == ']' {
            self.next_char();
            if self.peek() == Some(']') {
                self.next_char();
                return ShellToken::DoubleRBracket;
            }
            return ShellToken::RBracket;
        }

        // { could be brace expansion {a,b,c} or command grouping
        // If { is followed by non-whitespace, treat it as start of a word
        if c == '{' {
            self.next_char();
            match self.peek() {
                Some(' ') | Some('\t') | Some('\n') | None => {
                    // Whitespace after { means command grouping
                    return ShellToken::LBrace;
                }
                _ => {
                    // Content immediately after { - could be brace expansion
                    // Read the whole brace expression as a word
                    let mut word = String::from("{");
                    let mut depth = 1;

                    while let Some(ch) = self.peek() {
                        if ch == '{' {
                            depth += 1;
                            word.push(self.next_char().unwrap());
                        } else if ch == '}' {
                            depth -= 1;
                            word.push(self.next_char().unwrap());
                            if depth == 0 {
                                break;
                            }
                        } else if (ch == ' ' || ch == '\t' || ch == '\n') && depth == 1 {
                            // Whitespace at top level without closing - not valid brace expansion
                            // Return what we have and let next token handle the rest
                            break;
                        } else {
                            word.push(self.next_char().unwrap());
                        }
                    }

                    // Continue reading the rest of the word after the brace
                    while let Some(ch) = self.peek() {
                        if ch.is_whitespace()
                            || ch == ';'
                            || ch == '&'
                            || ch == '|'
                            || ch == '<'
                            || ch == '>'
                            || ch == '('
                            || ch == ')'
                        {
                            break;
                        }
                        word.push(self.next_char().unwrap());
                    }

                    return ShellToken::Word(word);
                }
            }
        }

        if c == '}' {
            self.next_char();
            return ShellToken::RBrace;
        }

        if c == '!' {
            self.next_char();
            if self.peek() == Some('=') {
                self.next_char();
                return ShellToken::Word("!=".to_string());
            }
            if self.peek() == Some('(') {
                // !(pattern) extglob - read the whole pattern
                let mut word = String::from("!(");
                self.next_char(); // consume (
                let mut depth = 1;
                while let Some(ch) = self.peek() {
                    word.push(self.next_char().unwrap());
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                // Continue reading the rest of the word
                while let Some(ch) = self.peek() {
                    if ch.is_whitespace()
                        || ch == ';'
                        || ch == '&'
                        || ch == '|'
                        || ch == '<'
                        || ch == '>'
                    {
                        break;
                    }
                    word.push(self.next_char().unwrap());
                }
                return ShellToken::Word(word);
            }
            return ShellToken::Bang;
        }

        // Word (including keywords)
        if c.is_alphanumeric()
            || c == '_'
            || c == '/'
            || c == '.'
            || c == '-'
            || c == '$'
            || c == '\''
            || c == '"'
            || c == '~'
            || c == '*'
            || c == '?'
            || c == '%'
            || c == '+'
            || c == '@'
            || c == ':'
            || c == '='
        {
            return self.read_word();
        }

        // Unknown - consume and return as word
        self.next_char();
        ShellToken::Word(c.to_string())
    }

    fn read_process_sub(&mut self) -> String {
        let mut content = String::new();
        let mut depth = 1;

        while let Some(c) = self.next_char() {
            if c == '(' {
                depth += 1;
                content.push(c);
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    content.push(')');
                    break;
                }
                content.push(c);
            } else {
                content.push(c);
            }
        }

        content
    }

    fn read_heredoc(&mut self) -> ShellToken {
        // Skip optional whitespace before delimiter
        while self.peek() == Some(' ') || self.peek() == Some('\t') {
            self.next_char();
        }

        // Check for quoted delimiter (prevents variable expansion)
        let quoted = self.peek() == Some('\'') || self.peek() == Some('"');
        if quoted {
            self.next_char(); // consume opening quote
        }

        // Read delimiter
        let mut delimiter = String::new();
        while let Some(c) = self.peek() {
            if c == '\n' || c == ' ' || c == '\t' {
                break;
            }
            if quoted && (c == '\'' || c == '"') {
                self.next_char(); // consume closing quote
                break;
            }
            delimiter.push(self.next_char().unwrap());
        }

        // Skip to end of line
        while let Some(c) = self.peek() {
            if c == '\n' {
                self.next_char();
                break;
            }
            self.next_char();
        }

        // Read content until we find delimiter alone on a line
        let mut content = String::new();
        let mut current_line = String::new();

        while let Some(c) = self.next_char() {
            if c == '\n' {
                if current_line.trim() == delimiter {
                    // Found the end delimiter
                    break;
                }
                content.push_str(&current_line);
                content.push('\n');
                current_line.clear();
            } else {
                current_line.push(c);
            }
        }

        ShellToken::HereDoc(delimiter, content)
    }

    fn read_word(&mut self) -> ShellToken {
        let mut word = String::new();

        while let Some(c) = self.peek() {
            match c {
                // Word terminators (note: { and } handled specially for brace expansion)
                ' ' | '\t' | '\n' | ';' | '&' | '<' | '>' | '[' | ']' => break,

                // These could be word terminators, but check for extglob patterns first
                '|' | '(' | ')' => {
                    // If we have an extglob prefix right before (, consume the whole pattern
                    if c == '(' && !word.is_empty() {
                        let last_char = word.chars().last().unwrap();
                        if last_char == '?'
                            || last_char == '*'
                            || last_char == '+'
                            || last_char == '@'
                            || last_char == '!'
                        {
                            // This is an extglob pattern, consume until matching )
                            word.push(self.next_char().unwrap()); // (
                            let mut depth = 1;
                            while let Some(ch) = self.peek() {
                                word.push(self.next_char().unwrap());
                                if ch == '(' {
                                    depth += 1;
                                } else if ch == ')' {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                            }
                            continue;
                        }
                    }
                    // Otherwise, it's a terminator
                    break;
                }

                // Brace expansion: {a,b,c} or {1..10}
                // Include the whole brace group in the word
                '{' => {
                    word.push(self.next_char().unwrap()); // {
                    let mut depth = 1;

                    while let Some(ch) = self.peek() {
                        if ch == '{' {
                            depth += 1;
                            word.push(self.next_char().unwrap());
                        } else if ch == '}' {
                            depth -= 1;
                            word.push(self.next_char().unwrap());
                            if depth == 0 {
                                break;
                            }
                        } else if ch == ' ' || ch == '\t' || ch == '\n' {
                            // Whitespace inside braces - stop but keep what we have
                            break;
                        } else {
                            word.push(self.next_char().unwrap());
                        }
                    }
                }

                '}' => break, // Lone } is always a terminator

                // Variable expansion with ${...}
                '$' => {
                    word.push(self.next_char().unwrap());
                    if self.peek() == Some('{') {
                        // Handle ${...} including brackets inside
                        word.push(self.next_char().unwrap()); // {
                        let mut depth = 1;
                        while let Some(ch) = self.peek() {
                            if ch == '{' {
                                depth += 1;
                            } else if ch == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    word.push(self.next_char().unwrap());
                                    break;
                                }
                            }
                            word.push(self.next_char().unwrap());
                        }
                    } else if self.peek() == Some('(') {
                        // Handle $(...) or $((...))
                        word.push(self.next_char().unwrap()); // (
                        let mut depth = 1;
                        while let Some(ch) = self.peek() {
                            if ch == '(' {
                                depth += 1;
                            } else if ch == ')' {
                                depth -= 1;
                                if depth == 0 {
                                    word.push(self.next_char().unwrap());
                                    break;
                                }
                            }
                            word.push(self.next_char().unwrap());
                        }
                    }
                }

                // Array assignment: arr=(a b c)
                '=' => {
                    word.push(self.next_char().unwrap());
                    if self.peek() == Some('(') {
                        // Handle =(...) array literal
                        word.push(self.next_char().unwrap()); // (
                        let mut depth = 1;
                        while let Some(ch) = self.peek() {
                            if ch == '(' {
                                depth += 1;
                            } else if ch == ')' {
                                depth -= 1;
                                if depth == 0 {
                                    word.push(self.next_char().unwrap());
                                    break;
                                }
                            }
                            word.push(self.next_char().unwrap());
                        }
                    }
                }

                // Quotes
                '\'' => {
                    self.next_char();
                    while let Some(ch) = self.peek() {
                        if ch == '\'' {
                            self.next_char();
                            break;
                        }
                        word.push(self.next_char().unwrap());
                    }
                }
                '"' => {
                    self.next_char(); // consume opening "
                    while let Some(ch) = self.peek() {
                        if ch == '"' {
                            self.next_char(); // consume closing "
                            break;
                        }
                        if ch == '\\' {
                            self.next_char(); // consume backslash
                            if let Some(escaped) = self.next_char() {
                                word.push(escaped);
                            }
                        } else {
                            word.push(self.next_char().unwrap());
                        }
                    }
                }

                // Escape
                '\\' => {
                    self.next_char();
                    if let Some(escaped) = self.next_char() {
                        word.push(escaped);
                    }
                }

                // Regular character
                _ => {
                    word.push(self.next_char().unwrap());
                }
            }
        }

        // Check for keywords
        match word.as_str() {
            "if" => ShellToken::If,
            "then" => ShellToken::Then,
            "else" => ShellToken::Else,
            "elif" => ShellToken::Elif,
            "fi" => ShellToken::Fi,
            "case" => ShellToken::Case,
            "esac" => ShellToken::Esac,
            "for" => ShellToken::For,
            "while" => ShellToken::While,
            "until" => ShellToken::Until,
            "do" => ShellToken::Do,
            "done" => ShellToken::Done,
            "in" => ShellToken::In,
            "function" => ShellToken::Function,
            "select" => ShellToken::Select,
            "time" => ShellToken::Time,
            "coproc" => ShellToken::Coproc,
            _ => ShellToken::Word(word),
        }
    }
}

pub struct ShellParser<'a> {
    lexer: ShellLexer<'a>,
    current: ShellToken,
}

impl<'a> ShellParser<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut lexer = ShellLexer::new(input);
        let current = lexer.next_token();
        Self { lexer, current }
    }

    fn advance(&mut self) -> ShellToken {
        let old = std::mem::replace(&mut self.current, self.lexer.next_token());
        old
    }

    fn expect(&mut self, expected: ShellToken) -> Result<(), String> {
        if self.current == expected {
            self.advance();
            Ok(())
        } else {
            Err(format!("Expected {:?}, got {:?}", expected, self.current))
        }
    }

    fn skip_newlines(&mut self) {
        while self.current == ShellToken::Newline {
            self.advance();
        }
    }

    pub fn parse_script(&mut self) -> Result<Vec<ShellCommand>, String> {
        let mut commands = Vec::new();

        self.skip_newlines();

        while self.current != ShellToken::Eof {
            if let Some(cmd) = self.parse_complete_command()? {
                commands.push(cmd);
            }
            self.skip_newlines();
        }

        Ok(commands)
    }

    fn parse_complete_command(&mut self) -> Result<Option<ShellCommand>, String> {
        self.skip_newlines();

        if self.current == ShellToken::Eof {
            return Ok(None);
        }

        let cmd = self.parse_list()?;

        // Consume separator
        match &self.current {
            ShellToken::Newline | ShellToken::Semi | ShellToken::Amp => {
                self.advance();
            }
            _ => {}
        }

        Ok(Some(cmd))
    }

    fn parse_list(&mut self) -> Result<ShellCommand, String> {
        let first = self.parse_pipeline()?;
        let mut items = vec![(first, ListOp::Semi)];

        loop {
            let op = match &self.current {
                ShellToken::AmpAmp => ListOp::And,
                ShellToken::PipePipe => ListOp::Or,
                ShellToken::Semi => ListOp::Semi,
                ShellToken::Amp => ListOp::Amp,
                ShellToken::Newline => {
                    // Newline terminates the list, let parent handle it
                    break;
                }
                _ => break,
            };

            self.advance();
            self.skip_newlines();

            // Update the operator for the previous command
            if let Some(last) = items.last_mut() {
                last.1 = op;
            }

            if self.current == ShellToken::Eof
                || self.current == ShellToken::Then
                || self.current == ShellToken::Else
                || self.current == ShellToken::Elif
                || self.current == ShellToken::Fi
                || self.current == ShellToken::Do
                || self.current == ShellToken::Done
                || self.current == ShellToken::Esac
                || self.current == ShellToken::RBrace
                || self.current == ShellToken::RParen
            {
                break;
            }

            let next = self.parse_pipeline()?;
            items.push((next, ListOp::Semi));
        }

        if items.len() == 1 {
            let (cmd, op) = items.pop().unwrap();
            // If the op is significant (like &), we need to return a List
            if matches!(op, ListOp::Amp) {
                Ok(ShellCommand::List(vec![(cmd, op)]))
            } else {
                Ok(cmd)
            }
        } else {
            Ok(ShellCommand::List(items))
        }
    }

    fn parse_pipeline(&mut self) -> Result<ShellCommand, String> {
        let negated = if self.current == ShellToken::Bang {
            self.advance();
            true
        } else {
            false
        };

        let first = self.parse_command()?;
        let mut cmds = vec![first];

        while self.current == ShellToken::Pipe {
            self.advance();
            self.skip_newlines();
            cmds.push(self.parse_command()?);
        }

        if cmds.len() == 1 && !negated {
            Ok(cmds.pop().unwrap())
        } else {
            Ok(ShellCommand::Pipeline(cmds, negated))
        }
    }

    fn parse_command(&mut self) -> Result<ShellCommand, String> {
        match &self.current {
            ShellToken::If => self.parse_if(),
            ShellToken::For => self.parse_for(),
            ShellToken::While => self.parse_while(),
            ShellToken::Until => self.parse_until(),
            ShellToken::Case => self.parse_case(),
            ShellToken::LBrace => self.parse_brace_group(),
            ShellToken::LParen => self.parse_subshell(),
            ShellToken::DoubleLBracket => self.parse_cond_command(),
            ShellToken::DoubleLParen => self.parse_arith_command(),
            ShellToken::Function => self.parse_function(),
            ShellToken::Coproc => self.parse_coproc(),
            _ => self.parse_simple_command(),
        }
    }

    fn parse_simple_command(&mut self) -> Result<ShellCommand, String> {
        let mut cmd = SimpleCommand {
            assignments: Vec::new(),
            words: Vec::new(),
            redirects: Vec::new(),
        };

        // Parse assignments and words
        loop {
            match &self.current {
                ShellToken::Word(w) => {
                    // Check for assignment (VAR=value or arr=(a b c))
                    if cmd.words.is_empty() && w.contains('=') && !w.starts_with('=') {
                        if let Some(eq_pos) = w.find('=') {
                            let var = w[..eq_pos].to_string();
                            let val = w[eq_pos + 1..].to_string();
                            if var.chars().all(|c| c.is_alphanumeric() || c == '_') {
                                // Check for array assignment: arr=(...)
                                if val.starts_with('(') && val.ends_with(')') {
                                    let array_content = &val[1..val.len() - 1];
                                    let elements: Vec<ShellWord> = array_content
                                        .split_whitespace()
                                        .map(|s| ShellWord::Literal(s.to_string()))
                                        .collect();
                                    cmd.assignments
                                        .push((var, ShellWord::ArrayLiteral(elements)));
                                } else {
                                    cmd.assignments.push((var, ShellWord::Literal(val)));
                                }
                                self.advance();
                                continue;
                            }
                        }
                    }

                    cmd.words.push(self.parse_word()?);
                }

                // Redirections
                ShellToken::Less
                | ShellToken::Greater
                | ShellToken::GreaterGreater
                | ShellToken::LessAmp
                | ShellToken::GreaterAmp
                | ShellToken::LessLess
                | ShellToken::LessLessLess
                | ShellToken::LessGreater
                | ShellToken::GreaterPipe
                | ShellToken::AmpGreater
                | ShellToken::AmpGreaterGreater
                | ShellToken::HereDoc(_, _) => {
                    cmd.redirects.push(self.parse_redirect()?);
                }

                _ => break,
            }
        }

        // Check for function definition: name() { ... }
        if cmd.words.len() == 1 && self.current == ShellToken::LParen {
            if let ShellWord::Literal(name) = &cmd.words[0] {
                let name = name.clone();
                self.advance(); // (
                self.expect(ShellToken::RParen)?; // )
                self.skip_newlines();
                let body = self.parse_command()?;
                return Ok(ShellCommand::FunctionDef(name, Box::new(body)));
            }
        }

        Ok(ShellCommand::Simple(cmd))
    }

    fn parse_word(&mut self) -> Result<ShellWord, String> {
        if let ShellToken::Word(w) = self.advance() {
            Ok(ShellWord::Literal(w))
        } else {
            Err("Expected word".to_string())
        }
    }

    fn parse_redirect(&mut self) -> Result<Redirect, String> {
        // Check for HereDoc token first
        if let ShellToken::HereDoc(delimiter, content) = &self.current {
            let delimiter = delimiter.clone();
            let content = content.clone();
            self.advance();
            return Ok(Redirect {
                fd: None,
                op: RedirectOp::HereDoc,
                target: ShellWord::Literal(delimiter),
                heredoc_content: Some(content),
            });
        }

        let op = match self.advance() {
            ShellToken::Less => RedirectOp::Read,
            ShellToken::Greater => RedirectOp::Write,
            ShellToken::GreaterGreater => RedirectOp::Append,
            ShellToken::LessAmp => RedirectOp::DupRead,
            ShellToken::GreaterAmp => RedirectOp::DupWrite,
            ShellToken::LessLess => RedirectOp::HereDoc,
            ShellToken::LessLessLess => RedirectOp::HereString,
            ShellToken::LessGreater => RedirectOp::ReadWrite,
            ShellToken::GreaterPipe => RedirectOp::Clobber,
            ShellToken::AmpGreater => RedirectOp::WriteBoth,
            ShellToken::AmpGreaterGreater => RedirectOp::AppendBoth,
            _ => return Err("Expected redirect operator".to_string()),
        };

        let target = self.parse_word()?;

        Ok(Redirect {
            fd: None,
            op,
            target,
            heredoc_content: None,
        })
    }

    fn parse_if(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::If)?;
        self.skip_newlines();

        let mut conditions = Vec::new();

        // if condition
        let cond = self.parse_compound_list()?;
        self.expect(ShellToken::Then)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        conditions.push((cond, body));

        // elif parts
        while self.current == ShellToken::Elif {
            self.advance();
            self.skip_newlines();
            let cond = self.parse_compound_list()?;
            self.expect(ShellToken::Then)?;
            self.skip_newlines();
            let body = self.parse_compound_list()?;
            conditions.push((cond, body));
        }

        // else part
        let else_part = if self.current == ShellToken::Else {
            self.advance();
            self.skip_newlines();
            Some(self.parse_compound_list()?)
        } else {
            None
        };

        self.expect(ShellToken::Fi)?;

        Ok(ShellCommand::Compound(CompoundCommand::If {
            conditions,
            else_part,
        }))
    }

    fn parse_for(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::For)?;
        self.skip_newlines();

        // Check for C-style for (( ... ))
        if self.current == ShellToken::DoubleLParen {
            return self.parse_for_arith();
        }

        // Variable name
        let var = if let ShellToken::Word(w) = self.advance() {
            w
        } else {
            return Err("Expected variable name after 'for'".to_string());
        };

        self.skip_newlines();

        // Optional 'in words...'
        let words = if self.current == ShellToken::In {
            self.advance();
            let mut words = Vec::new();
            while let ShellToken::Word(_) = &self.current {
                words.push(self.parse_word()?);
            }
            Some(words)
        } else {
            None
        };

        // Skip separator
        match &self.current {
            ShellToken::Semi | ShellToken::Newline => {
                self.advance();
            }
            _ => {}
        }
        self.skip_newlines();

        self.expect(ShellToken::Do)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        self.expect(ShellToken::Done)?;

        Ok(ShellCommand::Compound(CompoundCommand::For {
            var,
            words,
            body,
        }))
    }

    fn parse_for_arith(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::DoubleLParen)?;

        // Parse init; cond; step - simplified, just collect as strings
        let mut parts = Vec::new();
        let mut current_part = String::new();
        let mut depth = 0;

        loop {
            match &self.current {
                ShellToken::DoubleRParen if depth == 0 => break,
                ShellToken::DoubleLParen => {
                    depth += 1;
                    current_part.push_str("((");
                    self.advance();
                }
                ShellToken::DoubleRParen => {
                    depth -= 1;
                    current_part.push_str("))");
                    self.advance();
                }
                ShellToken::Semi => {
                    parts.push(current_part.trim().to_string());
                    current_part = String::new();
                    self.advance();
                }
                ShellToken::Word(w) => {
                    current_part.push_str(w);
                    current_part.push(' ');
                    self.advance();
                }
                ShellToken::Less => {
                    current_part.push('<');
                    self.advance();
                }
                ShellToken::Greater => {
                    current_part.push('>');
                    self.advance();
                }
                ShellToken::LessLess => {
                    current_part.push_str("<<");
                    self.advance();
                }
                ShellToken::GreaterGreater => {
                    current_part.push_str(">>");
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        parts.push(current_part.trim().to_string());

        self.expect(ShellToken::DoubleRParen)?;
        self.skip_newlines();

        // do ... done or ; ... ;
        match &self.current {
            ShellToken::Semi | ShellToken::Newline => {
                self.advance();
            }
            _ => {}
        }
        self.skip_newlines();

        self.expect(ShellToken::Do)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        self.expect(ShellToken::Done)?;

        Ok(ShellCommand::Compound(CompoundCommand::ForArith {
            init: parts.get(0).cloned().unwrap_or_default(),
            cond: parts.get(1).cloned().unwrap_or_default(),
            step: parts.get(2).cloned().unwrap_or_default(),
            body,
        }))
    }

    fn parse_while(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::While)?;
        self.skip_newlines();
        let condition = self.parse_compound_list()?;
        self.expect(ShellToken::Do)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        self.expect(ShellToken::Done)?;

        Ok(ShellCommand::Compound(CompoundCommand::While {
            condition,
            body,
        }))
    }

    fn parse_until(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::Until)?;
        self.skip_newlines();
        let condition = self.parse_compound_list()?;
        self.expect(ShellToken::Do)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        self.expect(ShellToken::Done)?;

        Ok(ShellCommand::Compound(CompoundCommand::Until {
            condition,
            body,
        }))
    }

    fn parse_case(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::Case)?;
        self.skip_newlines();
        let word = self.parse_word()?;
        self.skip_newlines();
        self.expect(ShellToken::In)?;
        self.skip_newlines();

        let mut cases = Vec::new();

        while self.current != ShellToken::Esac {
            // Parse patterns
            let mut patterns = Vec::new();

            // Optional opening paren
            if self.current == ShellToken::LParen {
                self.advance();
            }

            loop {
                patterns.push(self.parse_word()?);
                if self.current == ShellToken::Pipe {
                    self.advance();
                } else {
                    break;
                }
            }

            self.expect(ShellToken::RParen)?;
            self.skip_newlines();

            // Parse commands
            let body = self.parse_compound_list()?;

            // Parse terminator
            let term = match &self.current {
                ShellToken::DoubleSemi => {
                    self.advance();
                    CaseTerminator::Break
                }
                ShellToken::SemiAmp => {
                    self.advance();
                    CaseTerminator::Fallthrough
                }
                ShellToken::SemiSemiAmp => {
                    self.advance();
                    CaseTerminator::Continue
                }
                _ => CaseTerminator::Break,
            };

            cases.push((patterns, body, term));
            self.skip_newlines();
        }

        self.expect(ShellToken::Esac)?;

        Ok(ShellCommand::Compound(CompoundCommand::Case {
            word,
            cases,
        }))
    }

    fn parse_brace_group(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::LBrace)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        self.expect(ShellToken::RBrace)?;

        Ok(ShellCommand::Compound(CompoundCommand::BraceGroup(body)))
    }

    fn parse_subshell(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::LParen)?;
        self.skip_newlines();
        let body = self.parse_compound_list()?;
        self.expect(ShellToken::RParen)?;

        Ok(ShellCommand::Compound(CompoundCommand::Subshell(body)))
    }

    fn parse_cond_command(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::DoubleLBracket)?;

        // Collect tokens until ]]
        let mut tokens: Vec<String> = Vec::new();
        while self.current != ShellToken::DoubleRBracket && self.current != ShellToken::Eof {
            if let ShellToken::Word(w) = &self.current {
                tokens.push(w.clone());
            }
            self.advance();
        }

        self.expect(ShellToken::DoubleRBracket)?;

        // Parse the condition
        let expr = self.parse_cond_tokens(&tokens)?;
        Ok(ShellCommand::Compound(CompoundCommand::Cond(expr)))
    }

    fn parse_cond_tokens(&self, tokens: &[String]) -> Result<CondExpr, String> {
        if tokens.is_empty() {
            return Ok(CondExpr::StringNonEmpty(ShellWord::Literal(String::new())));
        }

        // Handle negation
        if tokens[0] == "!" {
            let inner = self.parse_cond_tokens(&tokens[1..])?;
            return Ok(CondExpr::Not(Box::new(inner)));
        }

        // Handle binary operators (look for operator in the middle)
        for (i, tok) in tokens.iter().enumerate() {
            match tok.as_str() {
                "=" | "==" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::StringEqual(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "!=" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::StringNotEqual(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "=~" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::StringMatch(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "-eq" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::NumEqual(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "-ne" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::NumNotEqual(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "-lt" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::NumLess(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "-le" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::NumLessEqual(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "-gt" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::NumGreater(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "-ge" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::NumGreaterEqual(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "<" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::StringLess(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                ">" => {
                    let left = tokens[..i].join(" ");
                    let right = tokens[i + 1..].join(" ");
                    return Ok(CondExpr::StringGreater(
                        ShellWord::Literal(left),
                        ShellWord::Literal(right),
                    ));
                }
                "&&" => {
                    let left = self.parse_cond_tokens(&tokens[..i])?;
                    let right = self.parse_cond_tokens(&tokens[i + 1..])?;
                    return Ok(CondExpr::And(Box::new(left), Box::new(right)));
                }
                "||" => {
                    let left = self.parse_cond_tokens(&tokens[..i])?;
                    let right = self.parse_cond_tokens(&tokens[i + 1..])?;
                    return Ok(CondExpr::Or(Box::new(left), Box::new(right)));
                }
                _ => {}
            }
        }

        // Handle unary operators
        if tokens.len() >= 2 {
            let op = &tokens[0];
            let arg = tokens[1..].join(" ");
            match op.as_str() {
                "-e" => return Ok(CondExpr::FileExists(ShellWord::Literal(arg))),
                "-f" => return Ok(CondExpr::FileRegular(ShellWord::Literal(arg))),
                "-d" => return Ok(CondExpr::FileDirectory(ShellWord::Literal(arg))),
                "-L" | "-h" => return Ok(CondExpr::FileSymlink(ShellWord::Literal(arg))),
                "-r" => return Ok(CondExpr::FileReadable(ShellWord::Literal(arg))),
                "-w" => return Ok(CondExpr::FileWritable(ShellWord::Literal(arg))),
                "-x" => return Ok(CondExpr::FileExecutable(ShellWord::Literal(arg))),
                "-s" => return Ok(CondExpr::FileNonEmpty(ShellWord::Literal(arg))),
                "-z" => return Ok(CondExpr::StringEmpty(ShellWord::Literal(arg))),
                "-n" => return Ok(CondExpr::StringNonEmpty(ShellWord::Literal(arg))),
                _ => {}
            }
        }

        // Default: treat as non-empty string test
        let expr_str = tokens.join(" ");
        Ok(CondExpr::StringNonEmpty(ShellWord::Literal(expr_str)))
    }

    fn parse_arith_command(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::DoubleLParen)?;

        let mut expr = String::new();
        let mut depth = 1;

        while depth > 0 {
            match &self.current {
                ShellToken::DoubleLParen => {
                    depth += 1;
                    expr.push_str("((");
                }
                ShellToken::DoubleRParen => {
                    depth -= 1;
                    if depth > 0 {
                        expr.push_str("))");
                    }
                }
                ShellToken::Word(w) => {
                    expr.push_str(w);
                    expr.push(' ');
                }
                ShellToken::Less => expr.push('<'),
                ShellToken::Greater => expr.push('>'),
                ShellToken::LessLess => expr.push_str("<<"),
                ShellToken::GreaterGreater => expr.push_str(">>"),
                ShellToken::Bang => expr.push('!'),
                ShellToken::Pipe => expr.push('|'),
                ShellToken::AmpAmp => expr.push_str("&&"),
                ShellToken::PipePipe => expr.push_str("||"),
                ShellToken::Eof => break,
                _ => {}
            }
            self.advance();
        }

        Ok(ShellCommand::Compound(CompoundCommand::Arith(
            expr.trim().to_string(),
        )))
    }

    fn parse_function(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::Function)?;
        self.skip_newlines();

        let name = if let ShellToken::Word(w) = self.advance() {
            w
        } else {
            return Err("Expected function name".to_string());
        };

        self.skip_newlines();

        // Optional ()
        if self.current == ShellToken::LParen {
            self.advance();
            self.expect(ShellToken::RParen)?;
        }

        self.skip_newlines();
        let body = self.parse_command()?;

        Ok(ShellCommand::FunctionDef(name, Box::new(body)))
    }

    fn parse_coproc(&mut self) -> Result<ShellCommand, String> {
        self.expect(ShellToken::Coproc)?;

        // Check if next token is a name or a command
        let (name, body) = if let ShellToken::Word(w) = &self.current {
            // Check if it's NAME { ... } or just a simple command
            let word = w.clone();
            self.advance();

            if self.current == ShellToken::LBrace {
                // coproc NAME { ... }
                let cmd = self.parse_brace_group()?;
                (Some(word), cmd)
            } else {
                // coproc cmd args... - the word is the command name
                let mut words = vec![ShellWord::Literal(word)];

                while let ShellToken::Word(w) = &self.current {
                    words.push(ShellWord::Literal(w.clone()));
                    self.advance();
                }

                let cmd = ShellCommand::Simple(SimpleCommand {
                    assignments: Vec::new(),
                    words,
                    redirects: Vec::new(),
                });
                (None, cmd)
            }
        } else if self.current == ShellToken::LBrace {
            // coproc { ... }
            let cmd = self.parse_brace_group()?;
            (None, cmd)
        } else {
            return Err("Expected command or brace group after coproc".to_string());
        };

        Ok(ShellCommand::Compound(CompoundCommand::Coproc {
            name,
            body: Box::new(body),
        }))
    }

    fn parse_compound_list(&mut self) -> Result<Vec<ShellCommand>, String> {
        let mut cmds = Vec::new();

        self.skip_newlines();

        loop {
            // Check for terminators
            match &self.current {
                ShellToken::Then
                | ShellToken::Else
                | ShellToken::Elif
                | ShellToken::Fi
                | ShellToken::Do
                | ShellToken::Done
                | ShellToken::Esac
                | ShellToken::RBrace
                | ShellToken::RParen
                | ShellToken::DoubleSemi
                | ShellToken::SemiAmp
                | ShellToken::SemiSemiAmp
                | ShellToken::Eof => break,
                _ => {}
            }

            cmds.push(self.parse_list()?);

            match &self.current {
                ShellToken::Semi | ShellToken::Amp | ShellToken::Newline => {
                    self.advance();
                    self.skip_newlines();
                }
                _ => break,
            }
        }

        Ok(cmds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let mut parser = ShellParser::new("echo hello world");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_pipeline() {
        let mut parser = ShellParser::new("ls | grep foo | head -5");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
        if let ShellCommand::Pipeline(pipes, _) = &cmds[0] {
            assert_eq!(pipes.len(), 3);
        } else {
            panic!("Expected pipeline");
        }
    }

    #[test]
    fn test_if_statement() {
        let mut parser = ShellParser::new("if true; then echo yes; fi");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_for_loop() {
        let mut parser = ShellParser::new("for i in 1 2 3; do echo $i; done");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_while_loop() {
        let mut parser = ShellParser::new("while true; do echo loop; done");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_and_or_list() {
        let mut parser = ShellParser::new("cmd1 && cmd2 || cmd3");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn test_function_def() {
        let mut parser = ShellParser::new("foo() { echo bar; }");
        let cmds = parser.parse_script().unwrap();
        assert_eq!(cmds.len(), 1);
        if let ShellCommand::FunctionDef(name, _) = &cmds[0] {
            assert_eq!(name, "foo");
        } else {
            panic!("Expected function def");
        }
    }
}
