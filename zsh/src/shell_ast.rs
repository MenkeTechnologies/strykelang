//! Zsh/Bash compatible shell parser for zshrs
//!
//! Parses POSIX sh, bash, and zsh syntax into an AST that can be executed.

use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum ShellToken {
    Word(String),
    SingleQuotedWord(String),
    DoubleQuotedWord(String),
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
    Typeset, // typeset, local, declare, export, readonly, integer, float

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
    // Zsh-style parameter expansion flags
    ZshFlags(Vec<ZshParamFlag>), // ${(flags)var}
}

/// Zsh parameter expansion flags
#[derive(Debug, Clone)]
pub enum ZshParamFlag {
    Lower,                 // L - lowercase
    Upper,                 // U - uppercase
    Capitalize,            // C - capitalize words
    Join(String),          // j:sep: - join array with separator
    JoinNewline,           // F - join with newlines
    Split(String),         // s:sep: - split string into array
    SplitLines,            // f - split on newlines
    SplitWords,            // z - split into words (shell parsing)
    Type,                  // t - type of variable
    Words,                 // w - word splitting
    Quote,                 // q - quote result
    DoubleQuote,           // qq - double quote
    QuoteBackslash,        // b - quote with backslashes for patterns
    Unique,                // u - unique elements only
    Reverse,               // O - reverse sort
    Sort,                  // o - sort
    NumericSort,           // n - numeric sort
    IndexSort,             // a - sort in array index order
    Keys,                  // k - associative array keys
    Values,                // v - associative array values
    Length,                // # - length (character codes)
    CountChars,            // c - count total characters
    Expand,                // e - perform shell expansions
    PromptExpand,          // % - expand prompt escapes
    PromptExpandFull,      // %% - full prompt expansion
    Visible,               // V - make non-printable chars visible
    Directory,             // D - substitute directory names
    Head(usize),           // [1,n] - first n elements
    Tail(usize),           // [-n,-1] - last n elements
    PadLeft(usize, char),  // l:len:fill: - pad left
    PadRight(usize, char), // r:len:fill: - pad right
    Width(usize),          // m - use width for padding
    Match,                 // M - include matched portion
    Remove,                // R - include non-matched portion (complement of M)
    Subscript,             // S - subscript scanning
    Parameter,             // P - use value as parameter name (indirection)
    Glob,                  // ~ - glob patterns in pattern
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
    pub assignments: Vec<(String, ShellWord, bool)>, // (name, value, is_append)
    pub words: Vec<ShellWord>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub fd: Option<i32>,
    pub op: RedirectOp,
    pub target: ShellWord,
    pub heredoc_content: Option<String>,
    pub fd_var: Option<String>, // For {varname}>file syntax
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    WithRedirects(Box<ShellCommand>, Vec<Redirect>),
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

    fn skip_whitespace(&mut self) -> bool {
        let mut had_whitespace = false;
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.next_char();
                had_whitespace = true;
            } else if c == '\\' {
                // Line continuation
                self.next_char();
                if self.peek() == Some('\n') {
                    self.next_char();
                }
                had_whitespace = true;
            } else {
                break;
            }
        }
        had_whitespace
    }

    fn skip_comment(&mut self, after_whitespace: bool) {
        // In shell, # only starts a comment when it's after whitespace or at start of line
        // Inside words or after non-whitespace, # is just a regular character
        if after_whitespace && self.peek() == Some('#') {
            while let Some(c) = self.peek() {
                if c == '\n' {
                    break;
                }
                self.next_char();
            }
        }
    }

    pub fn next_token(&mut self) -> ShellToken {
        let was_at_line_start = self.at_line_start;
        
        let had_whitespace = self.skip_whitespace();
        // Comments start with # when at line start or after whitespace
        self.skip_comment(had_whitespace || was_at_line_start);

        let c = match self.peek() {
            Some(c) => c,
            None => return ShellToken::Eof,
        };

        // Newline
        if c == '\n' {
            self.next_char();
            self.at_line_start = true;
            return ShellToken::Newline;
        }
        
        // No longer at line start
        self.at_line_start = false;

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
            // Check if this looks like a glob/character class [abc] vs test command
            // If next char is alphanumeric and we can find a ] before whitespace, it's a glob
            if let Some(next_ch) = self.peek() {
                if !next_ch.is_whitespace() && next_ch != ']' {
                    // Could be a glob pattern - consume until ]
                    let mut pattern = String::from("[");
                    while let Some(ch) = self.peek() {
                        pattern.push(self.next_char().unwrap());
                        if ch == ']' {
                            // Check if more word characters follow
                            while let Some(c2) = self.peek() {
                                if c2.is_whitespace() || c2 == ';' || c2 == '&' || c2 == '|' 
                                   || c2 == '<' || c2 == '>' || c2 == ')' || c2 == '\n' {
                                    break;
                                }
                                pattern.push(self.next_char().unwrap());
                            }
                            return ShellToken::Word(pattern);
                        }
                        if ch.is_whitespace() {
                            // Hit whitespace before ] - revert to returning [
                            // We've already consumed some chars, so return what we have
                            return ShellToken::Word(pattern);
                        }
                    }
                    return ShellToken::Word(pattern);
                }
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
                ' ' | '\t' | '\n' | ';' | '&' | '<' | '>' => break,
                
                // [ can be a test command at word start, or a glob character class
                // If immediately followed by non-space, consume as glob pattern
                '[' => {
                    // Always consume [ and look for matching ]
                    word.push(self.next_char().unwrap()); // [
                    let mut bracket_depth = 1;
                    while let Some(ch) = self.peek() {
                        word.push(self.next_char().unwrap());
                        if ch == '[' {
                            bracket_depth += 1;
                        } else if ch == ']' {
                            bracket_depth -= 1;
                            if bracket_depth == 0 {
                                break;
                            }
                        }
                        // If we hit whitespace before ], this wasn't a valid bracket expr
                        if ch == ' ' || ch == '\t' || ch == '\n' {
                            break;
                        }
                    }
                    continue;
                }
                // ] at word start breaks, otherwise it's part of the word (shouldn't happen if [ is handled)
                ']' => {
                    if word.is_empty() {
                        break;
                    }
                    word.push(self.next_char().unwrap());
                    continue;
                }

                // These could be word terminators, but check for extglob patterns first
                '|' | '(' | ')' => {
                    if c == '(' && !word.is_empty() {
                        let last_char = word.chars().last().unwrap();
                        // Check for extglob pattern
                        if matches!(last_char, '?' | '*' | '+' | '@' | '!') {
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
                        // Check for array assignment: var=(...)
                        if last_char == '=' {
                            // Array literal - consume until matching )
                            word.push(self.next_char().unwrap()); // (
                            let mut depth = 1;
                            let mut in_sq = false;
                            let mut in_dq = false;
                            while let Some(ch) = self.peek() {
                                if in_sq {
                                    if ch == '\'' {
                                        in_sq = false;
                                    }
                                    word.push(self.next_char().unwrap());
                                } else if in_dq {
                                    if ch == '"' {
                                        in_dq = false;
                                    } else if ch == '\\' {
                                        word.push(self.next_char().unwrap());
                                        if self.peek().is_some() {
                                            word.push(self.next_char().unwrap());
                                        }
                                        continue;
                                    }
                                    word.push(self.next_char().unwrap());
                                } else {
                                    match ch {
                                        '\'' => {
                                            in_sq = true;
                                            word.push(self.next_char().unwrap());
                                        }
                                        '"' => {
                                            in_dq = true;
                                            word.push(self.next_char().unwrap());
                                        }
                                        '(' => {
                                            depth += 1;
                                            word.push(self.next_char().unwrap());
                                        }
                                        ')' => {
                                            depth -= 1;
                                            word.push(self.next_char().unwrap());
                                            if depth == 0 {
                                                break;
                                            }
                                        }
                                        '\\' => {
                                            word.push(self.next_char().unwrap());
                                            if self.peek().is_some() {
                                                word.push(self.next_char().unwrap());
                                            }
                                        }
                                        _ => word.push(self.next_char().unwrap()),
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

                // Variable expansion with ${...} or $'...' ANSI-C quoting
                '$' => {
                    word.push(self.next_char().unwrap());
                    if self.peek() == Some('\'') {
                        // $'...' ANSI-C quoting - content is literal with escape sequences
                        word.push(self.next_char().unwrap()); // '
                        while let Some(ch) = self.peek() {
                            if ch == '\'' {
                                word.push(self.next_char().unwrap());
                                break;
                            } else if ch == '\\' {
                                word.push(self.next_char().unwrap()); // \
                                if let Some(_escaped) = self.peek() {
                                    word.push(self.next_char().unwrap());
                                }
                            } else {
                                word.push(self.next_char().unwrap());
                            }
                        }
                    } else if self.peek() == Some('{') {
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

                // Single quotes - content is literal, no expansion
                '\'' => {
                    self.next_char();
                    while let Some(ch) = self.peek() {
                        if ch == '\'' {
                            self.next_char();
                            break;
                        }
                        let c = self.next_char().unwrap();
                        // Escape special chars that would otherwise be expanded
                        // Use \x00 prefix to mark chars that should be literal
                        // Include parens to prevent subshell/array interpretation
                        if matches!(c, '`' | '$' | '(' | ')') {
                            word.push('\x00');
                        }
                        word.push(c);
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
                            if let Some(escaped) = self.peek() {
                                // In double quotes, only these chars are special after backslash:
                                // $, `, ", \, and newline
                                match escaped {
                                    '$' | '`' | '"' | '\\' | '\n' => {
                                        word.push(self.next_char().unwrap());
                                    }
                                    _ => {
                                        // Keep the backslash for other chars
                                        word.push('\\');
                                        word.push(self.next_char().unwrap());
                                    }
                                }
                            } else {
                                word.push('\\');
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
            // Typeset-like reserved words (zsh/Src/hashtable.c lines 1083-1105)
            "typeset" | "local" | "declare" | "export" | "readonly" 
                | "integer" | "float" => ShellToken::Typeset,
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

    /// Parse array elements, properly handling quotes
    fn parse_array_elements(content: &str) -> Vec<ShellWord> {
        let mut elements = Vec::new();
        let mut current = String::new();
        let mut chars = content.chars().peekable();
        let mut in_single_quote = false;
        let mut in_double_quote = false;

        while let Some(c) = chars.next() {
            if in_single_quote {
                if c == '\'' {
                    in_single_quote = false;
                    // Single-quoted string - mark special chars with \x00
                    let marked: String = current
                        .chars()
                        .flat_map(|ch| {
                            if matches!(ch, '`' | '$' | '(' | ')') {
                                vec!['\x00', ch]
                            } else {
                                vec![ch]
                            }
                        })
                        .collect();
                    elements.push(ShellWord::Literal(marked));
                    current.clear();
                } else {
                    current.push(c);
                }
            } else if in_double_quote {
                if c == '"' {
                    in_double_quote = false;
                    elements.push(ShellWord::Literal(current.clone()));
                    current.clear();
                } else if c == '\\' {
                    if let Some(&next) = chars.peek() {
                        if matches!(next, '$' | '`' | '"' | '\\') {
                            chars.next();
                            current.push(next);
                        } else {
                            current.push(c);
                        }
                    } else {
                        current.push(c);
                    }
                } else {
                    current.push(c);
                }
            } else {
                match c {
                    '\'' => in_single_quote = true,
                    '"' => in_double_quote = true,
                    ' ' | '\t' | '\n' => {
                        if !current.is_empty() {
                            elements.push(ShellWord::Literal(current.clone()));
                            current.clear();
                        }
                    }
                    '\\' => {
                        if let Some(next) = chars.next() {
                            current.push(next);
                        }
                    }
                    _ => current.push(c),
                }
            }
        }

        // Push any remaining content
        if !current.is_empty() {
            elements.push(ShellWord::Literal(current));
        }

        elements
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
    
    /// Skip separators (newlines and semicolons) - matches zsh C's SEPER token
    fn skip_separators(&mut self) {
        while self.current == ShellToken::Newline || self.current == ShellToken::Semi {
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
        let cmd = match &self.current {
            ShellToken::If => self.parse_if(),
            ShellToken::For => self.parse_for(),
            ShellToken::While => self.parse_while(),
            ShellToken::Until => self.parse_until(),
            ShellToken::Case => self.parse_case(),
            ShellToken::LBrace => self.parse_brace_group(),
            ShellToken::LParen => {
                // Check if this is () { ... } anonymous function or ( ... ) subshell
                self.advance(); // consume (
                if self.current == ShellToken::RParen {
                    // Could be () followed by { ... } (anonymous function)
                    self.advance(); // consume )
                    self.skip_newlines();
                    if self.current == ShellToken::LBrace {
                        // This is an anonymous function () { ... }
                        let body = self.parse_brace_group()?;
                        Ok(ShellCommand::FunctionDef(String::new(), Box::new(body)))
                    } else {
                        // Just () - empty subshell, return empty command
                        Ok(ShellCommand::Compound(CompoundCommand::Subshell(vec![])))
                    }
                } else {
                    // Regular subshell ( ... )
                    self.skip_newlines();
                    let body = self.parse_compound_list()?;
                    self.expect(ShellToken::RParen)?;
                    Ok(ShellCommand::Compound(CompoundCommand::Subshell(body)))
                }
            },
            ShellToken::DoubleLBracket => self.parse_cond_command(),
            ShellToken::DoubleLParen => self.parse_arith_command(),
            ShellToken::Function => self.parse_function(),
            ShellToken::Coproc => self.parse_coproc(),
            _ => self.parse_simple_command(),
        }?;
        
        // Check for redirects after compound commands (e.g., { ... } 2>/dev/null)
        let mut redirects = Vec::new();
        loop {
            // Check for fd number followed by redirect
            if let ShellToken::Word(w) = &self.current {
                if w.chars().all(|c| c.is_ascii_digit()) {
                    let fd_str = w.clone();
                    self.advance();
                    match &self.current {
                        ShellToken::Less
                        | ShellToken::Greater
                        | ShellToken::GreaterGreater
                        | ShellToken::LessAmp
                        | ShellToken::GreaterAmp
                        | ShellToken::LessLess
                        | ShellToken::LessLessLess
                        | ShellToken::LessGreater
                        | ShellToken::GreaterPipe => {
                            let fd = fd_str.parse::<i32>().ok();
                            redirects.push(self.parse_redirect_with_fd(fd)?);
                            continue;
                        }
                        _ => {
                            // Not a redirect, this shouldn't happen in valid shell
                            // Put the token back conceptually by not consuming further
                            break;
                        }
                    }
                }
            }
            
            match &self.current {
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
                | ShellToken::AmpGreaterGreater => {
                    redirects.push(self.parse_redirect_with_fd(None)?);
                }
                _ => break,
            }
        }
        
        // If we have redirects, wrap the command in a compound with redirects
        if !redirects.is_empty() {
            // For now, just attach redirects to the command if it's compound
            // This is a simplification - proper handling would modify CompoundCommand
            Ok(ShellCommand::Compound(CompoundCommand::WithRedirects(
                Box::new(cmd),
                redirects,
            )))
        } else {
            Ok(cmd)
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
                    // Check for {varname} followed by redirect (e.g., {fd}>/dev/null)
                    // This is zsh's FD allocation syntax
                    if w.starts_with('{') && w.ends_with('}') && w.len() > 2 {
                        let varname = w[1..w.len() - 1].to_string();
                        if varname.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            let saved_word = w.clone();
                            self.advance(); // consume the {varname}
                            match &self.current {
                                ShellToken::Less
                                | ShellToken::Greater
                                | ShellToken::GreaterGreater
                                | ShellToken::LessAmp
                                | ShellToken::GreaterAmp
                                | ShellToken::LessLess
                                | ShellToken::LessLessLess
                                | ShellToken::LessGreater
                                | ShellToken::GreaterPipe => {
                                    // This is a redirect with fd variable allocation
                                    let mut redir = self.parse_redirect_with_fd(None)?;
                                    redir.fd_var = Some(varname);
                                    cmd.redirects.push(redir);
                                    continue;
                                }
                                _ => {
                                    // Not a redirect, treat as a regular word
                                    cmd.words.push(ShellWord::Literal(saved_word));
                                    continue;
                                }
                            }
                        }
                    }
                    
                    // Check for fd number followed by redirect (e.g., 2>/dev/null)
                    // Must check this BEFORE treating as a regular word
                    if w.chars().all(|c| c.is_ascii_digit()) {
                        let fd_str = w.clone();
                        self.advance(); // consume the number
                        match &self.current {
                            ShellToken::Less
                            | ShellToken::Greater
                            | ShellToken::GreaterGreater
                            | ShellToken::LessAmp
                            | ShellToken::GreaterAmp
                            | ShellToken::LessLess
                            | ShellToken::LessLessLess
                            | ShellToken::LessGreater
                            | ShellToken::GreaterPipe => {
                                let fd = fd_str.parse::<i32>().ok();
                                cmd.redirects.push(self.parse_redirect_with_fd(fd)?);
                                continue;
                            }
                            _ => {
                                // Not a redirect, treat as a regular word
                                cmd.words.push(ShellWord::Literal(fd_str));
                                continue;
                            }
                        }
                    }
                    
                    // Check for assignment (VAR=value, VAR+=value, arr=(a b c), arr[idx]=value)
                    if cmd.words.is_empty() && w.contains('=') && !w.starts_with('=') {
                        // Check for += (append) or = (assign)
                        let (eq_pos, is_append) = if let Some(pos) = w.find("+=") {
                            (pos, true)
                        } else if let Some(pos) = w.find('=') {
                            (pos, false)
                        } else {
                            (0, false) // won't happen due to contains check above
                        };
                        
                        if eq_pos > 0 {
                            let var = w[..eq_pos].to_string();
                            let val_start = if is_append { eq_pos + 2 } else { eq_pos + 1 };
                            let val = w[val_start..].to_string();
                            
                            // Check if var is valid: either simple name or name[subscript]
                            let is_valid_var = if let Some(bracket_pos) = var.find('[') {
                                // Array element: name[subscript]
                                let name = &var[..bracket_pos];
                                let rest = &var[bracket_pos..];
                                name.chars().all(|c| c.is_alphanumeric() || c == '_')
                                    && rest.ends_with(']')
                            } else {
                                // Simple variable
                                var.chars().all(|c| c.is_alphanumeric() || c == '_')
                            };
                            if is_valid_var {
                                // Check for array assignment: arr=(...)
                                if val.starts_with('(') && val.ends_with(')') {
                                    let array_content = &val[1..val.len() - 1];
                                    let elements = Self::parse_array_elements(array_content);
                                    cmd.assignments
                                        .push((var, ShellWord::ArrayLiteral(elements), is_append));
                                } else {
                                    cmd.assignments.push((var, ShellWord::Literal(val), is_append));
                                }
                                self.advance();
                                continue;
                            }
                        }
                    }

                    cmd.words.push(self.parse_word()?);
                }

                // [ as command (test builtin) or as argument (glob pattern)
                ShellToken::LBracket => {
                    // [ is always treated as a word (either as the test command or as glob pattern)
                    cmd.words.push(ShellWord::Literal("[".to_string()));
                    self.advance();
                }
                // ] as argument (closing bracket or glob pattern)
                ShellToken::RBracket => {
                    // ] is treated as a word in argument position
                    if !cmd.words.is_empty() {
                        cmd.words.push(ShellWord::Literal("]".to_string()));
                        self.advance();
                    } else {
                        break;
                    }
                }
                // Keywords can be used as words in argument position (not command position)
                ShellToken::If
                | ShellToken::Then
                | ShellToken::Else
                | ShellToken::Elif
                | ShellToken::Fi
                | ShellToken::Case
                | ShellToken::Esac
                | ShellToken::For
                | ShellToken::While
                | ShellToken::Until
                | ShellToken::Do
                | ShellToken::Done
                | ShellToken::In
                | ShellToken::Function
                | ShellToken::Select
                | ShellToken::Time
                | ShellToken::Coproc => {
                    // Only allow keywords as arguments (not as command name)
                    if !cmd.words.is_empty() {
                        cmd.words.push(self.parse_word()?);
                    } else {
                        break;
                    }
                }

                // Typeset is special - it's both a command and affects parsing
                ShellToken::Typeset => {
                    if cmd.words.is_empty() {
                        // Typeset as command - parse it
                        cmd.words.push(ShellWord::Literal("typeset".to_string()));
                        self.advance();
                    } else {
                        // Typeset as argument 
                        cmd.words.push(self.parse_word()?);
                    }
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
                    cmd.redirects.push(self.parse_redirect_with_fd(None)?);
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
        let token = self.advance();
        match token {
            ShellToken::Word(w) => Ok(ShellWord::Literal(w)),
            // [ at word position - handle as glob pattern or test command
            ShellToken::LBracket => Ok(ShellWord::Literal("[".to_string())),
            // Keywords can be used as words in argument position
            ShellToken::If => Ok(ShellWord::Literal("if".to_string())),
            ShellToken::Then => Ok(ShellWord::Literal("then".to_string())),
            ShellToken::Else => Ok(ShellWord::Literal("else".to_string())),
            ShellToken::Elif => Ok(ShellWord::Literal("elif".to_string())),
            ShellToken::Fi => Ok(ShellWord::Literal("fi".to_string())),
            ShellToken::Case => Ok(ShellWord::Literal("case".to_string())),
            ShellToken::Esac => Ok(ShellWord::Literal("esac".to_string())),
            ShellToken::For => Ok(ShellWord::Literal("for".to_string())),
            ShellToken::While => Ok(ShellWord::Literal("while".to_string())),
            ShellToken::Until => Ok(ShellWord::Literal("until".to_string())),
            ShellToken::Do => Ok(ShellWord::Literal("do".to_string())),
            ShellToken::Done => Ok(ShellWord::Literal("done".to_string())),
            ShellToken::In => Ok(ShellWord::Literal("in".to_string())),
            ShellToken::Function => Ok(ShellWord::Literal("function".to_string())),
            ShellToken::Select => Ok(ShellWord::Literal("select".to_string())),
            ShellToken::Time => Ok(ShellWord::Literal("time".to_string())),
            ShellToken::Coproc => Ok(ShellWord::Literal("coproc".to_string())),
            ShellToken::Typeset => Ok(ShellWord::Literal("typeset".to_string())),
            _ => Err("Expected word".to_string()),
        }
    }

    fn parse_redirect(&mut self) -> Result<Redirect, String> {
        self.parse_redirect_with_fd(None)
    }

    fn parse_redirect_with_fd(&mut self, fd: Option<i32>) -> Result<Redirect, String> {
        // Check for HereDoc token first
        if let ShellToken::HereDoc(delimiter, content) = &self.current {
            let delimiter = delimiter.clone();
            let content = content.clone();
            self.advance();
            return Ok(Redirect {
                fd,
                op: RedirectOp::HereDoc,
                target: ShellWord::Literal(delimiter),
                heredoc_content: Some(content),
                fd_var: None,
            });
        }

        // Check for {varname}>file syntax
        let mut fd_var = None;
        if let ShellToken::Word(w) = &self.current {
            if w.starts_with('{') && w.ends_with('}') && w.len() > 2 {
                let varname = w[1..w.len() - 1].to_string();
                fd_var = Some(varname);
                self.advance();
            }
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
            fd,
            op,
            target,
            heredoc_content: None,
            fd_var,
        })
    }

    /// Parse if statement - ported from zsh parse.c par_if()
    fn parse_if(&mut self) -> Result<ShellCommand, String> {
        // Port of par_if from zsh/Src/parse.c lines 1411-1511
        let mut conditions = Vec::new();
        let mut else_part = None;
        let mut usebrace = false;
        
        // Main loop handles if/elif chain (C: for(;;) at line 1419)
        let mut xtok = self.current.clone();
        loop {
            // C line 1422-1425: if xtok == FI, break
            if xtok == ShellToken::Fi {
                self.advance();
                break;
            }
            
            // C line 1427: zshlex() - advance past if/elif
            self.advance();
            
            // C line 1428-1429: if xtok == ELSE, break to else handling
            if xtok == ShellToken::Else {
                break;
            }
            
            // C line 1430-1431: skip separators (SEPER = newline or semi)
            self.skip_separators();
            
            // C line 1432-1434: must be IF or ELIF
            if xtok != ShellToken::If && xtok != ShellToken::Elif {
                return Err(format!("Expected If or Elif, got {:?}", xtok));
            }
            
            // C line 1438: par_save_list - parse condition
            let cond = self.parse_compound_list_until(&[ShellToken::Then, ShellToken::LBrace])?;
            
            // C line 1444-1445: skip separators after condition  
            self.skip_separators();
            
            // C line 1446: default expectation is FI
            xtok = ShellToken::Fi;
            
            // C line 1448-1456: THEN case
            if self.current == ShellToken::Then {
                usebrace = false;
                self.advance();
                let body = self.parse_compound_list()?;
                conditions.push((cond, body));
            }
            // C line 1457-1473: INBRACE case
            else if self.current == ShellToken::LBrace {
                usebrace = true;
                self.advance();
                self.skip_separators();
                let body = self.parse_compound_list_until(&[ShellToken::RBrace])?;
                if self.current != ShellToken::RBrace {
                    return Err(format!("Expected RBrace, got {:?}", self.current));
                }
                conditions.push((cond, body));
                // C line 1469: zshlex() - advance past }
                self.advance();
                // C line 1471-1472: if SEPER follows }, break out
                if self.current == ShellToken::Newline || self.current == ShellToken::Semi {
                    break;
                }
            }
            // C line 1477-1483: SHORTLOOPS - single command body (not implemented, error)
            else {
                return Err(format!("Expected Then or LBrace after condition, got {:?}", self.current));
            }
            
            // Check for elif/else/fi to continue loop
            xtok = self.current.clone();
            if xtok != ShellToken::Elif && xtok != ShellToken::Else && xtok != ShellToken::Fi {
                break;
            }
        }
        
        // C line 1487-1509: handle else
        if xtok == ShellToken::Else || self.current == ShellToken::Else {
            if self.current == ShellToken::Else {
                self.advance();
            }
            // C line 1490-1491: skip separators (SEPER)
            self.skip_separators();
            
            // C line 1492-1498: brace-style else
            if self.current == ShellToken::LBrace && usebrace {
                self.advance();
                self.skip_separators();
                let body = self.parse_compound_list_until(&[ShellToken::RBrace])?;
                if self.current != ShellToken::RBrace {
                    return Err(format!("Expected RBrace in else, got {:?}", self.current));
                }
                self.advance();
                else_part = Some(body);
            }
            // C line 1499-1504: traditional else, expect FI
            else {
                let body = self.parse_compound_list()?;
                if self.current != ShellToken::Fi {
                    return Err(format!("Expected Fi, got {:?}", self.current));
                }
                self.advance();
                else_part = Some(body);
            }
        }
        
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

        // Variable name - C line 1117-1118
        let var = if let ShellToken::Word(w) = self.advance() {
            w
        } else {
            return Err("Expected variable name after 'for'".to_string());
        };

        // C line 1142-1143: skip newlines after variable name
        while self.current == ShellToken::Newline {
            self.advance();
        }

        // zsh supports two syntaxes:
        // for var in words; do body; done
        // for var ( words ) { body }
        
        let words = if self.current == ShellToken::In {
            // Traditional syntax: for var in words (C line 1144-1152)
            self.advance();
            let mut words = Vec::new();
            while let ShellToken::Word(_) = &self.current {
                words.push(self.parse_word()?);
            }
            // C line 1149: expect SEPER after word list
            Some(words)
        } else if self.current == ShellToken::LParen {
            // zsh syntax: for var ( words ) (C line 1153-1162)
            self.advance();
            let mut words = Vec::new();
            while self.current != ShellToken::RParen && self.current != ShellToken::Eof {
                if let ShellToken::Word(_) = &self.current {
                    words.push(self.parse_word()?);
                } else if self.current == ShellToken::Newline {
                    self.advance(); // skip newlines inside ()
                } else {
                    break;
                }
            }
            self.expect(ShellToken::RParen)?;
            Some(words)
        } else {
            None
        };

        // C line 1167-1169: skip separators before body
        self.skip_separators();

        // Check for brace-style body { ... } or do-done style
        let body = if self.current == ShellToken::LBrace {
            // zsh { body } syntax (C line 1176-1183)
            self.advance();
            let body = self.parse_compound_list_until(&[ShellToken::RBrace])?;
            self.expect(ShellToken::RBrace)?;
            body
        } else {
            // Traditional do-done syntax (C line 1170-1175)
            self.expect(ShellToken::Do)?;
            let body = self.parse_compound_list()?;
            self.expect(ShellToken::Done)?;
            body
        };

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
        // Port of par_while from zsh/Src/parse.c lines 1521-1557
        self.expect(ShellToken::While)?;
        
        // C line 1528: par_save_list - parse condition  
        let condition = self.parse_compound_list_until(&[ShellToken::Do, ShellToken::LBrace])?;
        
        // C line 1530-1531: skip separators
        self.skip_separators();
        
        let body = if self.current == ShellToken::LBrace {
            // C line 1539-1545: INBRACE case
            self.advance();
            let body = self.parse_compound_list_until(&[ShellToken::RBrace])?;
            self.expect(ShellToken::RBrace)?;
            body
        } else {
            // C line 1532-1538: DOLOOP case
            self.expect(ShellToken::Do)?;
            let body = self.parse_compound_list()?;
            self.expect(ShellToken::Done)?;
            body
        };

        Ok(ShellCommand::Compound(CompoundCommand::While {
            condition,
            body,
        }))
    }

    fn parse_until(&mut self) -> Result<ShellCommand, String> {
        // Same as parse_while but for until
        self.expect(ShellToken::Until)?;
        
        let condition = self.parse_compound_list_until(&[ShellToken::Do, ShellToken::LBrace])?;
        
        self.skip_separators();
        
        let body = if self.current == ShellToken::LBrace {
            self.advance();
            let body = self.parse_compound_list_until(&[ShellToken::RBrace])?;
            self.expect(ShellToken::RBrace)?;
            body
        } else {
            self.expect(ShellToken::Do)?;
            let body = self.parse_compound_list()?;
            self.expect(ShellToken::Done)?;
            body
        };

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
            match &self.current {
                ShellToken::Word(w) => tokens.push(w.clone()),
                ShellToken::Bang => tokens.push("!".to_string()),
                ShellToken::AmpAmp => tokens.push("&&".to_string()),
                ShellToken::PipePipe => tokens.push("||".to_string()),
                ShellToken::LParen => tokens.push("(".to_string()),
                ShellToken::RParen => tokens.push(")".to_string()),
                ShellToken::Less => tokens.push("<".to_string()),
                ShellToken::Greater => tokens.push(">".to_string()),
                _ => {}
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
                ShellToken::LParen => expr.push('('),
                ShellToken::RParen => expr.push(')'),
                ShellToken::LBracket => expr.push('['),
                ShellToken::RBracket => expr.push(']'),
                ShellToken::Less => expr.push('<'),
                ShellToken::Greater => expr.push('>'),
                ShellToken::LessLess => expr.push_str("<<"),
                ShellToken::GreaterGreater => expr.push_str(">>"),
                ShellToken::Bang => expr.push('!'),
                ShellToken::Pipe => expr.push('|'),
                ShellToken::AmpAmp => expr.push_str("&&"),
                ShellToken::PipePipe => expr.push_str("||"),
                ShellToken::Semi => expr.push(';'),
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

    fn parse_compound_list_until(&mut self, stop_tokens: &[ShellToken]) -> Result<Vec<ShellCommand>, String> {
        let mut cmds = Vec::new();

        self.skip_newlines();

        loop {
            // Check for stop tokens
            if stop_tokens.contains(&self.current) {
                break;
            }
            
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

    fn parse_compound_list(&mut self) -> Result<Vec<ShellCommand>, String> {
        self.parse_compound_list_until(&[])
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

#[test]
fn test_echo_keyword_args() {
    let mut parser = ShellParser::new("echo done");
    let cmds = parser.parse_script().unwrap();
    assert_eq!(cmds.len(), 1);
    if let ShellCommand::Simple(cmd) = &cmds[0] {
        assert_eq!(cmd.words.len(), 2);
    } else {
        panic!("Expected simple command");
    }
}
