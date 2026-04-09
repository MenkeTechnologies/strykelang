/// AST node types for the Perl 5 interpreter.
/// Every node carries a `line` field for error reporting.

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub struct Statement {
    pub kind: StmtKind,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Expression(Expr),
    If {
        condition: Expr,
        body: Block,
        elsifs: Vec<(Expr, Block)>,
        else_block: Option<Block>,
    },
    Unless {
        condition: Expr,
        body: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
        label: Option<String>,
    },
    Until {
        condition: Expr,
        body: Block,
        label: Option<String>,
    },
    DoWhile {
        body: Block,
        condition: Expr,
    },
    For {
        init: Option<Box<Statement>>,
        condition: Option<Expr>,
        step: Option<Expr>,
        body: Block,
        label: Option<String>,
    },
    Foreach {
        var: String,
        list: Expr,
        body: Block,
        label: Option<String>,
    },
    SubDecl {
        name: String,
        params: Vec<String>,
        body: Block,
    },
    Package {
        name: String,
    },
    Use {
        module: String,
        imports: Vec<Expr>,
    },
    No {
        module: String,
        imports: Vec<Expr>,
    },
    Return(Option<Expr>),
    Last(Option<String>),
    Next(Option<String>),
    Redo(Option<String>),
    My(Vec<VarDecl>),
    Our(Vec<VarDecl>),
    Local(Vec<VarDecl>),
    /// `mysync $x = 0` — thread-safe atomic variable for parallel blocks
    MySync(Vec<VarDecl>),
    /// Bare block (for scoping or do {})
    Block(Block),
    /// `BEGIN { ... }`
    Begin(Block),
    /// `END { ... }`
    End(Block),
    /// Empty statement (bare semicolon)
    Empty,
}

#[derive(Debug, Clone)]
pub struct VarDecl {
    pub sigil: Sigil,
    pub name: String,
    pub initializer: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sigil {
    Scalar,
    Array,
    Hash,
}

pub type Block = Vec<Statement>;

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    // Literals
    Integer(i64),
    Float(f64),
    String(String),
    Regex(String, String),
    QW(Vec<String>),
    Undef,

    // Interpolated string (mix of literal and variable parts)
    InterpolatedString(Vec<StringPart>),

    // Variables
    ScalarVar(String),
    ArrayVar(String),
    HashVar(String),
    ArrayElement {
        array: String,
        index: Box<Expr>,
    },
    HashElement {
        hash: String,
        key: Box<Expr>,
    },
    ArraySlice {
        array: String,
        indices: Vec<Expr>,
    },
    HashSlice {
        hash: String,
        keys: Vec<Expr>,
    },

    // References
    ScalarRef(Box<Expr>),
    ArrayRef(Vec<Expr>),
    HashRef(Vec<(Expr, Expr)>),
    CodeRef {
        params: Vec<String>,
        body: Block,
    },
    Deref {
        expr: Box<Expr>,
        kind: Sigil,
    },
    ArrowDeref {
        expr: Box<Expr>,
        index: Box<Expr>,
        kind: DerefKind,
    },

    // Operators
    BinOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    PostfixOp {
        expr: Box<Expr>,
        op: PostfixOp,
    },
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    CompoundAssign {
        target: Box<Expr>,
        op: BinOp,
        value: Box<Expr>,
    },
    Ternary {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },

    // String repetition: "abc" x 3
    Repeat {
        expr: Box<Expr>,
        count: Box<Expr>,
    },

    // Range: 1..10
    Range {
        from: Box<Expr>,
        to: Box<Expr>,
    },

    // Function call
    FuncCall {
        name: String,
        args: Vec<Expr>,
    },

    // Method call: $obj->method(args)
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },

    // Special forms
    Print {
        handle: Option<String>,
        args: Vec<Expr>,
    },
    Say {
        handle: Option<String>,
        args: Vec<Expr>,
    },
    Printf {
        handle: Option<String>,
        args: Vec<Expr>,
    },
    Die(Vec<Expr>),
    Warn(Vec<Expr>),

    // Regex operations
    Match {
        expr: Box<Expr>,
        pattern: String,
        flags: String,
        /// When true, `/g` uses Perl scalar semantics (one match per eval, updates `pos`).
        scalar_g: bool,
    },
    Substitution {
        expr: Box<Expr>,
        pattern: String,
        replacement: String,
        flags: String,
    },
    Transliterate {
        expr: Box<Expr>,
        from: String,
        to: String,
        flags: String,
    },

    // List operations
    MapExpr {
        block: Block,
        list: Box<Expr>,
    },
    GrepExpr {
        block: Block,
        list: Box<Expr>,
    },
    SortExpr {
        cmp: Option<Block>,
        list: Box<Expr>,
    },
    ReverseExpr(Box<Expr>),
    JoinExpr {
        separator: Box<Expr>,
        list: Box<Expr>,
    },
    SplitExpr {
        pattern: Box<Expr>,
        string: Box<Expr>,
        limit: Option<Box<Expr>>,
    },

    // Parallel extensions
    PMapExpr {
        block: Block,
        list: Box<Expr>,
    },
    PGrepExpr {
        block: Block,
        list: Box<Expr>,
    },
    PForExpr {
        block: Block,
        list: Box<Expr>,
    },
    PSortExpr {
        cmp: Option<Block>,
        list: Box<Expr>,
    },
    /// `preduce { $a + $b } @list` — parallel fold/reduce using rayon.
    /// $a and $b are set to the accumulator and current element.
    PReduceExpr {
        block: Block,
        list: Box<Expr>,
    },
    /// `fan COUNT { BLOCK }` — execute BLOCK across all cores COUNT times.
    /// $_ is set to the iteration index (0..COUNT-1).
    FanExpr {
        count: Box<Expr>,
        block: Block,
    },

    // Array/Hash operations
    Push {
        array: Box<Expr>,
        values: Vec<Expr>,
    },
    Pop(Box<Expr>),
    Shift(Box<Expr>),
    Unshift {
        array: Box<Expr>,
        values: Vec<Expr>,
    },
    Splice {
        array: Box<Expr>,
        offset: Option<Box<Expr>>,
        length: Option<Box<Expr>>,
        replacement: Vec<Expr>,
    },
    Delete(Box<Expr>),
    Exists(Box<Expr>),
    Keys(Box<Expr>),
    Values(Box<Expr>),
    Each(Box<Expr>),

    // String operations
    Chomp(Box<Expr>),
    Chop(Box<Expr>),
    Length(Box<Expr>),
    Substr {
        string: Box<Expr>,
        offset: Box<Expr>,
        length: Option<Box<Expr>>,
        replacement: Option<Box<Expr>>,
    },
    Index {
        string: Box<Expr>,
        substr: Box<Expr>,
        position: Option<Box<Expr>>,
    },
    Rindex {
        string: Box<Expr>,
        substr: Box<Expr>,
        position: Option<Box<Expr>>,
    },
    Sprintf {
        format: Box<Expr>,
        args: Vec<Expr>,
    },

    // Numeric
    Abs(Box<Expr>),
    Int(Box<Expr>),
    Sqrt(Box<Expr>),
    Sin(Box<Expr>),
    Cos(Box<Expr>),
    Atan2 {
        y: Box<Expr>,
        x: Box<Expr>,
    },
    Exp(Box<Expr>),
    Log(Box<Expr>),
    /// `rand` with optional upper bound (none = Perl default 1.0).
    Rand(Option<Box<Expr>>),
    /// `srand` with optional seed (none = time-based).
    Srand(Option<Box<Expr>>),
    Hex(Box<Expr>),
    Oct(Box<Expr>),

    // Case
    Lc(Box<Expr>),
    Uc(Box<Expr>),
    Lcfirst(Box<Expr>),
    Ucfirst(Box<Expr>),

    /// Unicode case fold (Perl `fc`).
    Fc(Box<Expr>),
    /// DES-style `crypt` (see libc `crypt(3)` on Unix; empty on other targets).
    Crypt {
        plaintext: Box<Expr>,
        salt: Box<Expr>,
    },
    /// `pos` — optional scalar lvalue target (`None` = `$_`).
    Pos(Option<Box<Expr>>),
    /// `study` — hint for repeated matching; returns byte length of the string.
    Study(Box<Expr>),

    // Type
    Defined(Box<Expr>),
    Ref(Box<Expr>),
    ScalarContext(Box<Expr>),

    // Char
    Chr(Box<Expr>),
    Ord(Box<Expr>),

    // I/O
    Open {
        handle: Box<Expr>,
        mode: Box<Expr>,
        file: Option<Box<Expr>>,
    },
    Close(Box<Expr>),
    ReadLine(Option<String>),
    Eof(Option<Box<Expr>>),

    Opendir {
        handle: Box<Expr>,
        path: Box<Expr>,
    },
    Readdir(Box<Expr>),
    Closedir(Box<Expr>),
    Rewinddir(Box<Expr>),
    Telldir(Box<Expr>),
    Seekdir {
        handle: Box<Expr>,
        position: Box<Expr>,
    },

    // File tests
    FileTest {
        op: char,
        expr: Box<Expr>,
    },

    // System
    System(Vec<Expr>),
    Exec(Vec<Expr>),
    Eval(Box<Expr>),
    Do(Box<Expr>),
    Require(Box<Expr>),
    Exit(Option<Box<Expr>>),
    Chdir(Box<Expr>),
    Mkdir {
        path: Box<Expr>,
        mode: Option<Box<Expr>>,
    },
    Unlink(Vec<Expr>),

    Stat(Box<Expr>),
    Lstat(Box<Expr>),
    Link {
        old: Box<Expr>,
        new: Box<Expr>,
    },
    Symlink {
        old: Box<Expr>,
        new: Box<Expr>,
    },
    Readlink(Box<Expr>),
    Glob(Vec<Expr>),

    // Bless
    Bless {
        ref_expr: Box<Expr>,
        class: Option<Box<Expr>>,
    },

    // Caller
    Caller(Option<Box<Expr>>),

    // Wantarray
    Wantarray,

    // List / Context
    List(Vec<Expr>),

    // Postfix if/unless/while/until/for
    PostfixIf {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    PostfixUnless {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    PostfixWhile {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    PostfixUntil {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    PostfixForeach {
        expr: Box<Expr>,
        list: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum StringPart {
    Literal(String),
    ScalarVar(String),
    ArrayVar(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DerefKind {
    Array,
    Hash,
    Call,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    NumEq,
    NumNe,
    NumLt,
    NumGt,
    NumLe,
    NumGe,
    Spaceship,
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    StrLe,
    StrGe,
    StrCmp,
    LogAnd,
    LogOr,
    DefinedOr,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    LogAndWord,
    LogOrWord,
    BindMatch,
    BindNotMatch,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Negate,
    LogNot,
    BitNot,
    LogNotWord,
    PreIncrement,
    PreDecrement,
    Ref,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PostfixOp {
    Increment,
    Decrement,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binop_deref_kind_distinct() {
        assert_ne!(BinOp::Add, BinOp::Sub);
        assert_eq!(DerefKind::Call, DerefKind::Call);
    }

    #[test]
    fn sigil_variants_exhaustive_in_tests() {
        let all = [Sigil::Scalar, Sigil::Array, Sigil::Hash];
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn program_empty_roundtrip_clone() {
        let p = Program { statements: vec![] };
        assert!(p.clone().statements.is_empty());
    }
}
