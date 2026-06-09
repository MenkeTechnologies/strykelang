/// `Token` — see variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    /// `Integer` variant.
    Integer(i64),
    /// `Float` variant.
    Float(f64),
    /// `SingleString` variant.
    SingleString(String),
    /// `DoubleString` variant.
    DoubleString(String),
    /// `` `...` `` or `qx{...}` — interpolated like double quotes, then executed as `sh -c` (Perl `qx`).
    BacktickString(String),
    /// Regex pattern: (pattern, flags, delimiter)
    Regex(String, String, char),
    /// `HereDoc` variant.
    HereDoc(String, String, bool),
    /// `QW` variant.
    QW(Vec<String>),

    // Variables
    /// `ScalarVar` variant.
    ScalarVar(String),
    /// `$$foo` — symbolic scalar deref (inner name is `foo` without sigil).
    DerefScalarVar(String),
    /// `ArrayVar` variant.
    ArrayVar(String),
    /// `HashVar` variant.
    HashVar(String),
    /// `ArrayAt` variant.
    ArrayAt,
    /// `HashPercent` variant.
    HashPercent,

    // Identifiers & keywords
    /// `Ident` variant.
    Ident(String),
    /// `Label` variant.
    Label(String),
    /// `PackageSep` variant.
    PackageSep,
    /// `format NAME =` … body … `.` (body lines without the closing `.`)
    FormatDecl {
        name: String,
        lines: Vec<String>,
    },

    // Arithmetic
    /// `Plus` variant.
    Plus,
    /// `Minus` variant.
    Minus,
    /// `Star` variant.
    Star,
    /// `Slash` variant.
    Slash,
    /// `Percent` variant.
    Percent,
    /// `Power` variant.
    Power,

    // String
    /// `Dot` variant.
    Dot,
    /// `X` variant.
    X,

    // Comparison (numeric)
    /// `NumEq` variant.
    NumEq,
    /// `NumNe` variant.
    NumNe,
    /// `NumLt` variant.
    NumLt,
    /// `NumGt` variant.
    NumGt,
    /// `NumLe` variant.
    NumLe,
    /// `NumGe` variant.
    NumGe,
    /// `Spaceship` variant.
    Spaceship,

    // Comparison (string)
    /// `StrEq` variant.
    StrEq,
    /// `StrNe` variant.
    StrNe,
    /// `StrLt` variant.
    StrLt,
    /// `StrGt` variant.
    StrGt,
    /// `StrLe` variant.
    StrLe,
    /// `StrGe` variant.
    StrGe,
    /// `StrCmp` variant.
    StrCmp,

    // Logical
    /// `LogAnd` variant.
    LogAnd,
    /// `LogOr` variant.
    LogOr,
    /// `LogNot` variant.
    LogNot,
    /// `LogAndWord` variant.
    LogAndWord,
    /// `LogOrWord` variant.
    LogOrWord,
    /// `LogNotWord` variant.
    LogNotWord,
    /// `DefinedOr` variant.
    DefinedOr,

    // Bitwise
    /// `BitAnd` variant.
    BitAnd,
    /// `BitOr` variant.
    BitOr,
    /// `BitXor` variant.
    BitXor,
    /// `BitNot` variant.
    BitNot,
    /// `ShiftLeft` variant.
    ShiftLeft,
    /// `ShiftRight` variant.
    ShiftRight,

    // Assignment
    /// `Assign` variant.
    Assign,
    /// `PlusAssign` variant.
    PlusAssign,
    /// `MinusAssign` variant.
    MinusAssign,
    /// `MulAssign` variant.
    MulAssign,
    /// `DivAssign` variant.
    DivAssign,
    /// `ModAssign` variant.
    ModAssign,
    /// `PowAssign` variant.
    PowAssign,
    /// `DotAssign` variant.
    DotAssign,
    /// `x=` — string-repetition compound assign (`$s x= 3`).
    XAssign,
    /// `AndAssign` variant.
    AndAssign,
    /// `OrAssign` variant.
    OrAssign,
    /// `XorAssign` variant.
    XorAssign,
    /// `ShiftLeftAssign` variant.
    ShiftLeftAssign,
    /// `ShiftRightAssign` variant.
    ShiftRightAssign,
    /// Bitwise `&=`
    BitAndAssign,
    /// Bitwise `|=`
    BitOrAssign,
    /// `DefinedOrAssign` variant.
    DefinedOrAssign,

    // Increment/Decrement
    /// `Increment` variant.
    Increment,
    /// `Decrement` variant.
    Decrement,

    // Regex binding
    /// `BindMatch` variant.
    BindMatch,
    /// `BindNotMatch` variant.
    BindNotMatch,

    // Arrows & separators
    /// `Arrow` variant.
    Arrow,
    /// `FatArrow` variant.
    FatArrow,
    /// `|>` — pipe-forward (F#/Elixir): `x |> f(a)` desugars to `f(x, a)` at parse time.
    PipeForward,
    /// `~>` — thread-first macro: `~> EXPR stage1 stage2 ...` injects as first arg
    ThreadArrow,
    /// `~>>` / `->>` — thread-last macro: injects as last arg
    ThreadArrowLast,
    /// `~s>` — streaming thread-first. Per-stage semantics match `~>`
    /// (insert threaded value as first arg / topic), but each stage runs
    /// in its own worker connected by bounded channels — items flow one
    /// at a time. Concurrent (per-item flow with backpressure), not
    /// chunk-parallel.
    ThreadArrowStream,
    /// `~s>>` — streaming thread-last. Per-stage semantics match `~>>`
    /// (insert threaded value as last arg).
    ThreadArrowStreamLast,
    /// `~p>` — parallel-chunk thread-first. Whole pipeline runs per chunk
    /// in parallel, results auto-merged at end (sugar for
    /// `par_reduce { stage1 |> stage2 |> ... } SOURCE`). `||>` or
    /// `|then|` switch from parallel-chunk back to pipe-forward / `~>`.
    ThreadArrowPar,
    /// `~p>>` — parallel-chunk thread-last counterpart of `~p>`.
    ThreadArrowParLast,
    /// `~d>` — **distributed** thread-first. Same chunk-block semantics as
    /// `~p>` (each stage operates on `@_` = chunk elements), but the chunks
    /// are shipped to remote workers on a cluster instead of local rayon
    /// threads. Syntax: `~d> on $cluster SOURCE stage1 stage2 ...`.
    /// Sugar for `dist_reduce on $cluster { stages } SOURCE`. Reuses the
    /// existing `pmap_on` dispatcher (one ssh process per slot, JOB frames
    /// flowing over a shared work queue, fault tolerance via retry).
    ThreadArrowDist,
    /// `~d>>` — distributed thread-last counterpart of `~d>` (insert threaded
    /// value as last positional arg to each named stage).
    ThreadArrowDistLast,
    /// Two-dot range / inclusive flip-flop (`..`).
    Range,
    /// Three-dot range / exclusive flip-flop (`...`); list expansion matches `..` (Perl).
    RangeExclusive,
    /// `Backslash` variant.
    Backslash,

    // Delimiters
    /// `LParen` variant.
    LParen,
    /// `RParen` variant.
    RParen,
    /// `LBracket` variant.
    LBracket,
    /// `RBracket` variant.
    RBracket,
    /// `LBrace` variant.
    LBrace,
    /// `RBrace` variant.
    RBrace,
    /// `>{` — standalone block in thread macro (not attached to a function)
    ArrowBrace,

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
    /// `is_term_start` — see implementation.
    pub fn is_term_start(&self) -> bool {
        matches!(
            self,
            Token::Integer(_)
                | Token::Float(_)
                | Token::SingleString(_)
                | Token::DoubleString(_)
                | Token::BacktickString(_)
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
                | Token::Regex(_, _, _)
                | Token::FileTest(_)
                | Token::ThreadArrow
                | Token::ThreadArrowLast
                | Token::ThreadArrowStream
                | Token::ThreadArrowStreamLast
                | Token::ThreadArrowPar
                | Token::ThreadArrowParLast
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
    "var",
    "val",
    "mysync",
    // `varsync` — Kotlin-style declarator alias for `mysync`, same
    // way `var` aliases `my`. Both spell the same lockless atomic
    // shared-state binding at the AST level (StmtKind::MySync).
    "varsync",
    "our",
    "oursync",
    "local",
    "sub",
    "fn",
    "struct",
    "enum",
    "class",
    "trait",
    "extends",
    "impl",
    "pub",
    "priv",
    "Self",
    "return",
    "if",
    "elsif",
    "else",
    "unless",
    "while",
    "until",
    // `loop { ... }` — Rust-style infinite loop, desugars to `while (1)`.
    "loop",
    "for",
    "foreach",
    "do",
    "last",
    "next",
    "redo",
    "use",
    // `import` — alias for `use`. Parses identically (same `parse_use`
    // path, same StmtKind variants). Lets Python/JS-shaped scripts spell
    // `import Foo;` while keeping `use Foo;` working.
    "import",
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
    "shuffle",
    "chunked",
    "windowed",
    "unshift",
    "splice",
    "split",
    "join",
    "json_decode",
    "json_encode",
    "json_jq",
    "jwt_decode",
    "jwt_decode_unsafe",
    "jwt_encode",
    "log_debug",
    "log_error",
    "log_info",
    "log_json",
    "log_level",
    "log_trace",
    "log_warn",
    "sha256",
    "sha1",
    "md5",
    "hmac_sha256",
    "hmac",
    "uuid",
    "base64_encode",
    "base64_decode",
    "hex_encode",
    "hex_decode",
    "gzip",
    "gunzip",
    "zstd",
    "zstd_decode",
    "datetime_utc",
    "datetime_from_epoch",
    "datetime_parse_rfc3339",
    "datetime_strftime",
    "toml_decode",
    "toml_encode",
    "yaml_decode",
    "yaml_encode",
    "url_encode",
    "url_decode",
    "uri_escape",
    "uri_unescape",
    "sort",
    "reverse",
    "reversed",
    "map",
    "maps",
    "flat_map",
    "flat_maps",
    "flatten",
    "compact",
    "reject",
    "grepv",
    "concat",
    "chain",
    "set",
    "list_count",
    "list_size",
    "count",
    "size",
    "cnt",
    "inject",
    "first",
    "detect",
    "find",
    "find_all",
    "match",
    "grep",
    "greps",
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
    "all",
    "any",
    "none",
    "take_while",
    "drop_while",
    "skip_while",
    "skip",
    "first_or",
    "tap",
    "peek",
    "with_index",
    "pmap",
    "pflat_map",
    "puniq",
    "pfirst",
    "pany",
    "pmap_chunked",
    "pipeline",
    "pgrep",
    "pfor",
    "par_lines",
    "par_walk",
    "pwatch",
    "psort",
    "reduce",
    "fold",
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
    "uniq",
    "distinct",
    "uniqstr",
    "uniqint",
    "uniqnum",
    "pairs",
    "unpairs",
    "pairkeys",
    "pairvalues",
    "pairgrep",
    "pairmap",
    "pairfirst",
    "sample",
    "zip",
    "zip_shortest",
    "mesh",
    "mesh_shortest",
    "notall",
    "reductions",
    "sum",
    "sum0",
    "product",
    "min",
    "max",
    "minstr",
    "maxstr",
    "mean",
    "median",
    "mode",
    "stddev",
    "variance",
    "async",
    "spawn",
    "trace",
    "timer",
    "bench",
    "await",
    "slurp",
    "swallow",
    "ingest",
    "burp",
    "god",
    "capture",
    "fetch_url",
    "fetch",
    "fetch_json",
    "fetch_async",
    "fetch_async_json",
    "json_jq",
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
    "thread",
    "t",
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
            keyword_or_ident("list_count"),
            Token::Ident(s) if s == "list_count"
        ));
        assert!(matches!(
            keyword_or_ident("list_size"),
            Token::Ident(s) if s == "list_size"
        ));
        assert!(matches!(keyword_or_ident("cnt"), Token::Ident(s) if s == "cnt"));
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
        assert!(matches!(keyword_or_ident("fold"), Token::Ident(s) if s == "fold"));
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
