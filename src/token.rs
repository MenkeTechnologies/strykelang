#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Integer(i64),
    Float(f64),
    SingleString(String),
    DoubleString(String),
    Regex(String, String),
    HereDoc(String, String),
    QW(Vec<String>),

    // Variables
    ScalarVar(String),
    ArrayVar(String),
    HashVar(String),
    ArrayAt,
    HashPercent,

    // Identifiers & keywords
    Ident(String),
    Label(String),
    PackageSep,

    // Arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power,

    // String
    Dot,
    X,

    // Comparison (numeric)
    NumEq,
    NumNe,
    NumLt,
    NumGt,
    NumLe,
    NumGe,
    Spaceship,

    // Comparison (string)
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    StrLe,
    StrGe,
    StrCmp,

    // Logical
    LogAnd,
    LogOr,
    LogNot,
    LogAndWord,
    LogOrWord,
    LogNotWord,
    DefinedOr,

    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    ShiftLeft,
    ShiftRight,

    // Assignment
    Assign,
    PlusAssign,
    MinusAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    DotAssign,
    AndAssign,
    OrAssign,
    XorAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
    DefinedOrAssign,

    // Increment/Decrement
    Increment,
    Decrement,

    // Regex binding
    BindMatch,
    BindNotMatch,

    // Arrows & separators
    Arrow,
    FatArrow,
    Range,
    Backslash,

    // Delimiters
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,

    // Punctuation
    Semicolon,
    Comma,
    Question,
    Colon,

    // I/O
    Diamond,
    ReadLine(String),

    // File tests
    FileTest(char),

    // Special
    Eof,
    Newline,
}

impl Token {
    pub fn is_term_start(&self) -> bool {
        matches!(
            self,
            Token::Integer(_)
                | Token::Float(_)
                | Token::SingleString(_)
                | Token::DoubleString(_)
                | Token::ScalarVar(_)
                | Token::ArrayVar(_)
                | Token::HashVar(_)
                | Token::Ident(_)
                | Token::LParen
                | Token::LBracket
                | Token::LBrace
                | Token::Backslash
                | Token::Minus
                | Token::LogNot
                | Token::BitNot
                | Token::LogNotWord
                | Token::QW(_)
                | Token::Regex(_, _)
                | Token::FileTest(_)
        )
    }
}

/// Resolve an identifier to a keyword token or leave as Ident.
pub fn keyword_or_ident(word: &str) -> Token {
    match word {
        "x" => Token::X,
        "eq" => Token::StrEq,
        "ne" => Token::StrNe,
        "lt" => Token::StrLt,
        "gt" => Token::StrGt,
        "le" => Token::StrLe,
        "ge" => Token::StrGe,
        "cmp" => Token::StrCmp,
        "and" => Token::LogAndWord,
        "or" => Token::LogOrWord,
        "not" => Token::LogNotWord,
        _ => Token::Ident(word.to_string()),
    }
}

/// All Perl keyword identifiers that are NOT converted to separate token variants.
/// The parser recognizes these as `Token::Ident("keyword")`.
pub const KEYWORDS: &[&str] = &[
    "my",
    "our",
    "local",
    "sub",
    "return",
    "if",
    "elsif",
    "else",
    "unless",
    "while",
    "until",
    "for",
    "foreach",
    "do",
    "last",
    "next",
    "redo",
    "use",
    "no",
    "require",
    "package",
    "bless",
    "print",
    "say",
    "die",
    "warn",
    "chomp",
    "chop",
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "split",
    "join",
    "sort",
    "reverse",
    "map",
    "grep",
    "keys",
    "values",
    "each",
    "delete",
    "exists",
    "open",
    "close",
    "read",
    "write",
    "seek",
    "tell",
    "eof",
    "defined",
    "undef",
    "ref",
    "eval",
    "exec",
    "system",
    "chdir",
    "mkdir",
    "rmdir",
    "unlink",
    "rename",
    "chmod",
    "chown",
    "length",
    "substr",
    "index",
    "rindex",
    "sprintf",
    "printf",
    "lc",
    "uc",
    "lcfirst",
    "ucfirst",
    "hex",
    "oct",
    "int",
    "abs",
    "sqrt",
    "scalar",
    "wantarray",
    "caller",
    "exit",
    "pos",
    "quotemeta",
    "chr",
    "ord",
    "pack",
    "unpack",
    "vec",
    "tie",
    "untie",
    "tied",
    "chomp",
    "chop",
    "defined",
    "dump",
    "each",
    "exists",
    "formline",
    "lock",
    "prototype",
    "reset",
    "scalar",
    "BEGIN",
    "END",
    "INIT",
    "CHECK",
    "UNITCHECK",
    "AUTOLOAD",
    "DESTROY",
    "pmap",
    "pgrep",
    "pfor",
    "psort",
];
