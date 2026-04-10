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
    /// `$$foo` — symbolic scalar deref (inner name is `foo` without sigil).
    DerefScalarVar(String),
    ArrayVar(String),
    HashVar(String),
    ArrayAt,
    HashPercent,

    // Identifiers & keywords
    Ident(String),
    Label(String),
    PackageSep,
    /// `format NAME =` … body … `.` (body lines without the closing `.`)
    FormatDecl {
        name: String,
        lines: Vec<String>,
    },

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
    /// Bitwise `&=`
    BitAndAssign,
    /// Bitwise `|=`
    BitOrAssign,
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
                | Token::DerefScalarVar(_)
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
    "frozen",
    "typed",
    "my",
    "mysync",
    "our",
    "local",
    "sub",
    "struct",
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
    "json_decode",
    "json_encode",
    "sort",
    "reverse",
    "map",
    "match",
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
    "pmap_chunked",
    "pipeline",
    "pgrep",
    "pfor",
    "par_lines",
    "par_walk",
    "pwatch",
    "psort",
    "reduce",
    "preduce",
    "preduce_init",
    "pmap_reduce",
    "pcache",
    "watch",
    "tie",
    "fan",
    "fan_cap",
    "pchannel",
    "pselect",
    "async",
    "spawn",
    "trace",
    "timer",
    "bench",
    "await",
    "slurp",
    "capture",
    "fetch_url",
    "fetch",
    "fetch_json",
    "fetch_async",
    "fetch_async_json",
    "par_fetch",
    "par_pipeline",
    "par_csv_read",
    "par_sed",
    "try",
    "catch",
    "finally",
    "given",
    "when",
    "default",
    "eval_timeout",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_or_ident_maps_string_ops() {
        assert!(matches!(keyword_or_ident("eq"), Token::StrEq));
        assert!(matches!(keyword_or_ident("cmp"), Token::StrCmp));
    }

    #[test]
    fn keyword_or_ident_non_keyword_is_ident() {
        assert!(matches!(
            keyword_or_ident("foo_bar"),
            Token::Ident(s) if s == "foo_bar"
        ));
    }

    #[test]
    fn keyword_or_ident_logical_words_and_repeat() {
        assert!(matches!(keyword_or_ident("and"), Token::LogAndWord));
        assert!(matches!(keyword_or_ident("or"), Token::LogOrWord));
        assert!(matches!(keyword_or_ident("not"), Token::LogNotWord));
        assert!(matches!(keyword_or_ident("x"), Token::X));
    }

    #[test]
    fn keyword_or_ident_string_comparison_words() {
        assert!(matches!(keyword_or_ident("lt"), Token::StrLt));
        assert!(matches!(keyword_or_ident("gt"), Token::StrGt));
        assert!(matches!(keyword_or_ident("ge"), Token::StrGe));
    }

    #[test]
    fn keyword_or_ident_string_le_ne() {
        assert!(matches!(keyword_or_ident("le"), Token::StrLe));
        assert!(matches!(keyword_or_ident("ne"), Token::StrNe));
    }

    #[test]
    fn keyword_or_ident_control_flow_keywords() {
        assert!(matches!(keyword_or_ident("if"), Token::Ident(s) if s == "if"));
        assert!(matches!(keyword_or_ident("else"), Token::Ident(s) if s == "else"));
        assert!(matches!(keyword_or_ident("elsif"), Token::Ident(s) if s == "elsif"));
        assert!(matches!(keyword_or_ident("unless"), Token::Ident(s) if s == "unless"));
        assert!(matches!(keyword_or_ident("while"), Token::Ident(s) if s == "while"));
        assert!(matches!(keyword_or_ident("until"), Token::Ident(s) if s == "until"));
        assert!(matches!(keyword_or_ident("for"), Token::Ident(s) if s == "for"));
        assert!(matches!(keyword_or_ident("foreach"), Token::Ident(s) if s == "foreach"));
        assert!(matches!(keyword_or_ident("return"), Token::Ident(s) if s == "return"));
    }

    #[test]
    fn keyword_or_ident_declarations() {
        assert!(matches!(keyword_or_ident("my"), Token::Ident(s) if s == "my"));
        assert!(matches!(keyword_or_ident("typed"), Token::Ident(s) if s == "typed"));
        assert!(matches!(keyword_or_ident("our"), Token::Ident(s) if s == "our"));
        assert!(matches!(keyword_or_ident("local"), Token::Ident(s) if s == "local"));
        assert!(matches!(keyword_or_ident("sub"), Token::Ident(s) if s == "sub"));
        assert!(matches!(keyword_or_ident("package"), Token::Ident(s) if s == "package"));
    }

    #[test]
    fn keyword_or_ident_io_and_list_ops() {
        assert!(matches!(keyword_or_ident("print"), Token::Ident(s) if s == "print"));
        assert!(matches!(keyword_or_ident("say"), Token::Ident(s) if s == "say"));
        assert!(matches!(keyword_or_ident("map"), Token::Ident(s) if s == "map"));
        assert!(matches!(keyword_or_ident("grep"), Token::Ident(s) if s == "grep"));
        assert!(matches!(keyword_or_ident("sort"), Token::Ident(s) if s == "sort"));
        assert!(matches!(keyword_or_ident("join"), Token::Ident(s) if s == "join"));
        assert!(matches!(keyword_or_ident("split"), Token::Ident(s) if s == "split"));
        assert!(matches!(
            keyword_or_ident("capture"),
            Token::Ident(s) if s == "capture"
        ));
    }

    #[test]
    fn keyword_or_ident_parallel_primitives() {
        assert!(matches!(keyword_or_ident("pmap"), Token::Ident(s) if s == "pmap"));
        assert!(matches!(
            keyword_or_ident("pmap_chunked"),
            Token::Ident(s) if s == "pmap_chunked"
        ));
        assert!(matches!(
            keyword_or_ident("pipeline"),
            Token::Ident(s) if s == "pipeline"
        ));
        assert!(matches!(keyword_or_ident("pgrep"), Token::Ident(s) if s == "pgrep"));
        assert!(matches!(keyword_or_ident("pfor"), Token::Ident(s) if s == "pfor"));
        assert!(matches!(keyword_or_ident("psort"), Token::Ident(s) if s == "psort"));
        assert!(matches!(keyword_or_ident("reduce"), Token::Ident(s) if s == "reduce"));
        assert!(matches!(keyword_or_ident("preduce"), Token::Ident(s) if s == "preduce"));
        assert!(matches!(keyword_or_ident("fan"), Token::Ident(s) if s == "fan"));
        assert!(matches!(keyword_or_ident("trace"), Token::Ident(s) if s == "trace"));
        assert!(matches!(keyword_or_ident("timer"), Token::Ident(s) if s == "timer"));
    }

    #[test]
    fn keyword_or_ident_type_and_ref() {
        assert!(matches!(keyword_or_ident("ref"), Token::Ident(s) if s == "ref"));
        assert!(matches!(keyword_or_ident("scalar"), Token::Ident(s) if s == "scalar"));
        assert!(matches!(keyword_or_ident("defined"), Token::Ident(s) if s == "defined"));
        assert!(matches!(keyword_or_ident("undef"), Token::Ident(s) if s == "undef"));
    }

    #[test]
    fn keyword_or_ident_block_hooks() {
        assert!(matches!(keyword_or_ident("BEGIN"), Token::Ident(s) if s == "BEGIN"));
        assert!(matches!(keyword_or_ident("END"), Token::Ident(s) if s == "END"));
        assert!(matches!(keyword_or_ident("INIT"), Token::Ident(s) if s == "INIT"));
    }

    #[test]
    fn keyword_or_ident_plain_identifier_untouched() {
        assert!(matches!(
            keyword_or_ident("xyzzy123"),
            Token::Ident(s) if s == "xyzzy123"
        ));
    }
}
