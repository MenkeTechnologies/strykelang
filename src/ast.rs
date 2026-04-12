//! AST node types for the Perl 5 interpreter.
//! Every node carries a `line` field for error reporting.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statement {
    /// Leading `LABEL:` on this statement (Perl convention: `FOO:`).
    pub label: Option<String>,
    pub kind: StmtKind,
    pub line: usize,
}

impl Statement {
    pub fn new(kind: StmtKind, line: usize) -> Self {
        Self {
            label: None,
            kind,
            line,
        }
    }
}

/// Named parameter in `sub name (SIG ...) { }` — perlrs extension (not Perl 5 prototype syntax).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubSigParam {
    /// `$name` — one positional scalar from `@_`.
    Scalar(String),
    /// `[ $a, @tail, ... ]` — next argument must be array-like; same element rules as algebraic `match`.
    ArrayDestruct(Vec<MatchArrayElem>),
    /// `{ k => $v, ... }` — next argument must be a hash or hashref; keys bind to listed scalars.
    HashDestruct(Vec<(String, String)>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        /// `while (...) { } continue { }`
        continue_block: Option<Block>,
    },
    Until {
        condition: Expr,
        body: Block,
        label: Option<String>,
        continue_block: Option<Block>,
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
        continue_block: Option<Block>,
    },
    Foreach {
        var: String,
        list: Expr,
        body: Block,
        label: Option<String>,
        continue_block: Option<Block>,
    },
    SubDecl {
        name: String,
        params: Vec<SubSigParam>,
        body: Block,
        /// Subroutine prototype text from `sub foo ($$) { }` (excluding parens).
        /// `None` when using structured [`SubSigParam`] signatures instead.
        prototype: Option<String>,
    },
    Package {
        name: String,
    },
    Use {
        module: String,
        imports: Vec<Expr>,
    },
    /// `use 5.008;` / `use 5;` — Perl version requirement (no-op at runtime in perlrs).
    UsePerlVersion {
        version: f64,
    },
    /// `use overload '""' => 'as_string', '+' => 'add';` — operator maps (method names in current package).
    UseOverload {
        pairs: Vec<(String, String)>,
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
    /// `state $x = 0` — persistent lexical variable (initialized once per sub)
    State(Vec<VarDecl>),
    /// `local $h{k}` / `local $SIG{__WARN__}` — lvalues that are not plain `my`-style names.
    LocalExpr {
        target: Expr,
        initializer: Option<Expr>,
    },
    /// `mysync $x = 0` — thread-safe atomic variable for parallel blocks
    MySync(Vec<VarDecl>),
    /// Bare block (for scoping or do {})
    Block(Block),
    /// Statements run in order without an extra scope frame (parser desugar).
    StmtGroup(Block),
    /// `BEGIN { ... }`
    Begin(Block),
    /// `END { ... }`
    End(Block),
    /// `UNITCHECK { ... }` — end of compilation unit (reverse order before CHECK).
    UnitCheck(Block),
    /// `CHECK { ... }` — end of compile phase (reverse order).
    Check(Block),
    /// `INIT { ... }` — before runtime main (forward order).
    Init(Block),
    /// Empty statement (bare semicolon)
    Empty,
    /// `goto EXPR` — expression evaluates to a label name in the same block.
    Goto {
        target: Box<Expr>,
    },
    /// Standalone `continue { BLOCK }` (normally follows a loop; parsed for acceptance).
    Continue(Block),
    /// `struct Name { field => Type, ... }` — fixed-field records (`Name->new`, `$x->field`).
    StructDecl {
        def: StructDef,
    },
    /// `eval_timeout SECS { ... }` — run block on a worker thread; main waits up to SECS (portable timeout).
    EvalTimeout {
        timeout: Expr,
        body: Block,
    },
    /// `try { } catch ($err) { } [ finally { } ]` — catch runtime/die errors (not `last`/`next`/`return` flow).
    /// `finally` runs after a successful `try` or after `catch` completes (including if `catch` rethrows).
    TryCatch {
        try_block: Block,
        catch_var: String,
        catch_block: Block,
        finally_block: Option<Block>,
    },
    /// `given (EXPR) { when ... default ... }` — topic in `$_`, `when` matches with regex / eq / smartmatch.
    Given {
        topic: Expr,
        body: Block,
    },
    /// `when (COND) { }` — only valid inside `given` (handled by given dispatcher).
    When {
        cond: Expr,
        body: Block,
    },
    /// `default { }` — only valid inside `given`.
    DefaultCase {
        body: Block,
    },
    /// `tie %hash` / `tie @arr` / `tie $x` — TIEHASH / TIEARRAY / TIESCALAR (FETCH/STORE).
    Tie {
        target: TieTarget,
        class: Expr,
        args: Vec<Expr>,
    },
    /// `format NAME =` picture/value lines … `.` — report templates for `write`.
    FormatDecl {
        name: String,
        lines: Vec<String>,
    },
}

/// Target of `tie` (hash, array, or scalar).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TieTarget {
    Hash(String),
    Array(String),
    Scalar(String),
}

/// Optional type for `typed my $x : Int` — enforced at assignment time (runtime).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerlTypeName {
    Int,
    Str,
    Float,
}

/// Compile-time record type: `struct Name { field => Type, ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, PerlTypeName)>,
}

impl StructDef {
    #[inline]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }
}

impl PerlTypeName {
    /// Bytecode encoding for `DeclareScalarTyped` / VM.
    #[inline]
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Int),
            1 => Some(Self::Str),
            2 => Some(Self::Float),
            _ => None,
        }
    }

    #[inline]
    pub fn as_byte(self) -> u8 {
        match self {
            Self::Int => 0,
            Self::Str => 1,
            Self::Float => 2,
        }
    }

    /// Strict runtime check: `Int` only integer-like [`PerlValue`](crate::value::PerlValue), `Str` only string, `Float` allows int or float.
    pub fn check_value(self, v: &crate::value::PerlValue) -> Result<(), String> {
        match self {
            Self::Int => {
                if v.is_integer_like() {
                    Ok(())
                } else {
                    Err(format!("expected Int (INTEGER), got {}", v.type_name()))
                }
            }
            Self::Str => {
                if v.is_string_like() {
                    Ok(())
                } else {
                    Err(format!("expected Str (STRING), got {}", v.type_name()))
                }
            }
            Self::Float => {
                if v.is_integer_like() || v.is_float_like() {
                    Ok(())
                } else {
                    Err(format!(
                        "expected Float (INTEGER or FLOAT), got {}",
                        v.type_name()
                    ))
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarDecl {
    pub sigil: Sigil,
    pub name: String,
    pub initializer: Option<Expr>,
    /// Set by `frozen my ...` — reassignments are rejected at compile time (bytecode) or runtime.
    pub frozen: bool,
    /// Set by `typed my $x : Int` (scalar only).
    pub type_annotation: Option<PerlTypeName>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Sigil {
    Scalar,
    Array,
    Hash,
    /// `local *FH` — filehandle slot alias (limited typeglob).
    Typeglob,
}

pub type Block = Vec<Statement>;

/// Comparator for `sort` — `{ $a <=> $b }`, or a code ref / expression (Perl `sort $cmp LIST`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortComparator {
    Block(Block),
    Code(Box<Expr>),
}

// ── Algebraic `match` expression (perlrs extension) ──

/// One arm of [`ExprKind::AlgebraicMatch`]: `PATTERN [if EXPR] => EXPR`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    /// Optional guard (`if EXPR`) evaluated after pattern match; `$_` is the match subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<Box<Expr>>,
    pub body: Expr,
}

/// `retry { } backoff => exponential` — sleep policy between attempts (after failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryBackoff {
    /// No delay between attempts.
    None,
    /// Delay grows linearly: `base_ms * attempt` (attempt starts at 1).
    Linear,
    /// Delay doubles each failure: `base_ms * 2^(attempt-1)` (capped).
    Exponential,
}

/// Pattern for algebraic `match` (distinct from the `=~` / regex [`ExprKind::Match`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchPattern {
    /// `_` — matches anything.
    Any,
    /// `/regex/` — subject stringified; on success the arm body sets `$_` to the subject and
    /// populates match variables (`$1`…, `$&`, `${^MATCH}`, `@-`/`@+`, `%+`, …) like `=~`.
    Regex { pattern: String, flags: String },
    /// Arbitrary expression compared for equality / smart-match against the subject.
    Value(Box<Expr>),
    /// `[1, 2, *]` — prefix elements match; optional `*` matches any tail (must be last).
    Array(Vec<MatchArrayElem>),
    /// `{ name => $n, ... }` — required keys; `$n` binds the value for the arm body.
    Hash(Vec<MatchHashPair>),
    /// `Some($x)` — matches array-like values with **at least two** elements where index `1` is
    /// Perl-truthy (perlrs: `$gen->next` yields `[value, more]` with `more` truthy while iterating).
    OptionSome(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchArrayElem {
    Expr(Expr),
    /// `$name` at the top of a pattern element — bind this position to a new lexical `$name`.
    /// Use `[($x)]` if you need smartmatch against the current value of `$x` instead.
    CaptureScalar(String),
    /// Rest-of-array wildcard (only valid as the last element).
    Rest,
    /// `@name` — bind remaining elements as a new array to `@name` (only valid as the last element).
    RestBind(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchHashPair {
    /// `key => _` — key must exist.
    KeyOnly { key: Expr },
    /// `key => $name` — key must exist; value is bound to `$name` in the arm.
    Capture { key: Expr, name: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MagicConstKind {
    /// Current source path (`$0`-style script name or `-e`).
    File,
    /// Line number of this token (1-based, same as lexer).
    Line,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprKind {
    // Literals
    Integer(i64),
    Float(f64),
    String(String),
    /// Unquoted identifier used as an expression term (`if (FOO)`), distinct from quoted `'FOO'` / `"FOO"`.
    /// Resolved at runtime: nullary subroutine if defined, otherwise stringifies like Perl barewords.
    Bareword(String),
    Regex(String, String),
    QW(Vec<String>),
    Undef,
    /// `__FILE__` / `__LINE__` (Perl compile-time literals).
    MagicConst(MagicConstKind),

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
    /// `@$container{keys}` — hash slice when the hash is reached via a scalar ref (Perl `@$href{k1,k2}`).
    HashSliceDeref {
        container: Box<Expr>,
        keys: Vec<Expr>,
    },
    /// `(LIST)[i,...]` / `(sort ...)[0]` — subscript after a non-arrow container (not `$a[i]` / `$r->[i]`).
    AnonymousListSlice {
        source: Box<Expr>,
        indices: Vec<Expr>,
    },

    // References
    ScalarRef(Box<Expr>),
    ArrayRef(Vec<Expr>),
    HashRef(Vec<(Expr, Expr)>),
    CodeRef {
        params: Vec<SubSigParam>,
        body: Block,
    },
    /// Unary `&name` — invoke subroutine `name` (Perl `&foo` / `&Foo::bar`).
    SubroutineRef(String),
    /// `\&name` — coderef to an existing named subroutine (Perl `\&foo`).
    SubroutineCodeRef(String),
    /// `\&{ EXPR }` — coderef to a subroutine whose name is given by `EXPR` (string or expression).
    DynamicSubCodeRef(Box<Expr>),
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

    // Range: `1..10` / `1...10` — in scalar context, `...` is the exclusive flip-flop (Perl `sed`-style).
    Range {
        from: Box<Expr>,
        to: Box<Expr>,
        #[serde(default)]
        exclusive: bool,
    },

    // Function call
    FuncCall {
        name: String,
        args: Vec<Expr>,
    },

    // Method call: $obj->method(args) or $obj->SUPER::method(args)
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        /// When true, dispatch starts after the caller package in the linearized MRO.
        #[serde(default)]
        super_call: bool,
    },
    /// Call through a coderef or invokable scalar: `$cr->(...)` is [`MethodCall`]; this is
    /// `$coderef(...)` or `&$coderef(...)` (the latter sets `ampersand`).
    IndirectCall {
        target: Box<Expr>,
        args: Vec<Expr>,
        #[serde(default)]
        ampersand: bool,
        /// True for unary `&$cr` with no `(...)` — Perl passes the caller's `@_` to the invoked sub.
        #[serde(default)]
        pass_caller_arglist: bool,
    },
    /// Limited typeglob: `*FOO` → handle name `FOO` for `open` / I/O.
    Typeglob(String),
    /// `*{ EXPR }` — typeglob slot by dynamic name (e.g. `*{$pkg . '::import'}`).
    TypeglobExpr(Box<Expr>),

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
        /// `flat_map { }` — peel one ARRAY ref from each iteration (perlrs extension).
        flatten_array_refs: bool,
    },
    /// `map EXPR, LIST` — EXPR is evaluated in list context with `$_` set to each element.
    MapExprComma {
        expr: Box<Expr>,
        list: Box<Expr>,
        flatten_array_refs: bool,
    },
    GrepExpr {
        block: Block,
        list: Box<Expr>,
    },
    /// `grep EXPR, LIST` — EXPR is evaluated with `$_` set to each element (Perl list vs scalar context).
    GrepExprComma {
        expr: Box<Expr>,
        list: Box<Expr>,
    },
    /// `sort BLOCK LIST`, `sort SUB LIST`, or `sort $coderef LIST` (Perl uses `$a`/`$b` in the comparator).
    SortExpr {
        cmp: Option<SortComparator>,
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
    /// `each { BLOCK } @list` — execute BLOCK for each element
    /// with `$_` aliased; void context (returns count in scalar context).
    ForEachExpr {
        block: Block,
        list: Box<Expr>,
    },

    // Parallel extensions
    PMapExpr {
        block: Block,
        list: Box<Expr>,
        /// `pmap { } @list, progress => EXPR` — when truthy, print a progress bar on stderr.
        progress: Option<Box<Expr>>,
        /// `pflat_map { }` — flatten each block result like [`ExprKind::MapExpr`] (arrays expand);
        /// parallel output is stitched in **input order** (unlike plain `pmap`, which is unordered).
        flat_outputs: bool,
        /// `pmap_on $cluster { } @list` — fan out over SSH (`pe --remote-worker`); `None` = local rayon.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_cluster: Option<Box<Expr>>,
    },
    /// `pmap_chunked N { BLOCK } @list [, progress => EXPR]` — parallel map in batches of N.
    PMapChunkedExpr {
        chunk_size: Box<Expr>,
        block: Block,
        list: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    PGrepExpr {
        block: Block,
        list: Box<Expr>,
        /// `pgrep { } @list, progress => EXPR` — stderr progress bar when truthy.
        progress: Option<Box<Expr>>,
    },
    /// `pfor { BLOCK } @list [, progress => EXPR]` — stderr progress bar when truthy.
    PForExpr {
        block: Block,
        list: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `par_lines PATH, sub { ... } [, progress => EXPR]` — optional stderr progress (per line).
    ParLinesExpr {
        path: Box<Expr>,
        callback: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `par_walk PATH, sub { ... } [, progress => EXPR]` — parallel recursive directory walk; `$_` is each path.
    ParWalkExpr {
        path: Box<Expr>,
        callback: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `pwatch GLOB, sub { ... }` — notify-based watcher (tree-walker only).
    PwatchExpr {
        path: Box<Expr>,
        callback: Box<Expr>,
    },
    /// `psort { } @list [, progress => EXPR]` — stderr progress when truthy (start/end phases).
    PSortExpr {
        cmp: Option<Block>,
        list: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `reduce { $a + $b } @list` — sequential left fold (like `List::Util::reduce`).
    /// `$a` is the accumulator; `$b` is the next list element.
    ReduceExpr {
        block: Block,
        list: Box<Expr>,
    },
    /// `preduce { $a + $b } @list` — parallel fold/reduce using rayon.
    /// $a and $b are set to the accumulator and current element.
    PReduceExpr {
        block: Block,
        list: Box<Expr>,
        /// `preduce { } @list, progress => EXPR` — stderr progress bar when truthy.
        progress: Option<Box<Expr>>,
    },
    /// `preduce_init EXPR, { $a / $b } @list` — parallel fold with explicit identity.
    /// Each chunk starts from a clone of `EXPR`; partials are merged (hash maps add counts per key;
    /// other types use the same block with `$a` / `$b` as partial accumulators). `$a` is the
    /// accumulator, `$b` is the next list element; `@_` is `($a, $b)` for `my ($acc, $item) = @_`.
    PReduceInitExpr {
        init: Box<Expr>,
        block: Block,
        list: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `pmap_reduce { map } { reduce } @list` — fused parallel map + tree reduce (no full mapped array).
    PMapReduceExpr {
        map_block: Block,
        reduce_block: Block,
        list: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `pcache { BLOCK } @list [, progress => EXPR]` — stderr progress bar when truthy.
    PcacheExpr {
        block: Block,
        list: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `pselect($rx1, $rx2, ...)` — optional `timeout => SECS` for bounded wait.
    PselectExpr {
        receivers: Vec<Expr>,
        timeout: Option<Box<Expr>>,
    },
    /// `fan [COUNT] { BLOCK }` — execute BLOCK COUNT times in parallel (default COUNT = rayon pool size).
    /// `fan_cap [COUNT] { BLOCK }` — same, but return value is a **list** of each block's return value (index order).
    /// `$_` is set to the iteration index (0..COUNT-1).
    /// Optional `, progress => EXPR` — stderr progress bar (like `pmap`).
    FanExpr {
        count: Option<Box<Expr>>,
        block: Block,
        progress: Option<Box<Expr>>,
        capture: bool,
    },

    /// `async { BLOCK }` — run BLOCK on a worker thread; returns a task handle.
    AsyncBlock {
        body: Block,
    },
    /// `spawn { BLOCK }` — same as [`ExprKind::AsyncBlock`] (Rust `thread::spawn`–style naming); join with `await`.
    SpawnBlock {
        body: Block,
    },
    /// `trace { BLOCK }` — print `mysync` scalar mutations to stderr (for parallel debugging).
    Trace {
        body: Block,
    },
    /// `timer { BLOCK }` — run BLOCK and return elapsed wall time in milliseconds (float).
    Timer {
        body: Block,
    },
    /// `bench { BLOCK } N` — run BLOCK `N` times (warmup + min/mean/p99 wall time, ms).
    Bench {
        body: Block,
        times: Box<Expr>,
    },
    /// `await EXPR` — join an async task, or return EXPR unchanged.
    Await(Box<Expr>),
    /// Read entire file as UTF-8 (`slurp $path`).
    Slurp(Box<Expr>),
    /// Run shell command and return structured output (`capture "cmd"`).
    Capture(Box<Expr>),
    /// `` `cmd` `` / `qx{cmd}` — run via `sh -c`, return **stdout as a string** (Perl); updates `$?`.
    Qx(Box<Expr>),
    /// Blocking HTTP GET (`fetch_url $url`).
    FetchUrl(Box<Expr>),

    /// `pchannel()` — unbounded; `pchannel(N)` — bounded capacity N.
    Pchannel {
        capacity: Option<Box<Expr>>,
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
    /// `open my $fh` — only valid as [`ExprKind::Open::handle`]; declares `$fh` and binds the handle.
    OpenMyHandle {
        name: String,
    },
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
    Rename {
        old: Box<Expr>,
        new: Box<Expr>,
    },
    /// `chmod MODE, @files` — first expr is mode, rest are paths.
    Chmod(Vec<Expr>),
    /// `chown UID, GID, @files` — first two are uid/gid, rest are paths.
    Chown(Vec<Expr>),

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
    /// Parallel recursive glob (rayon); same patterns as `glob`, different walk strategy.
    /// Optional `, progress => EXPR` — stderr progress bar (one tick per pattern).
    GlobPar {
        args: Vec<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `par_sed PATTERN, REPLACEMENT, FILES... [, progress => EXPR]` — parallel in-place regex replace per file (`g` semantics).
    ParSed {
        args: Vec<Expr>,
        progress: Option<Box<Expr>>,
    },

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

    /// `retry { BLOCK } times => N [, backoff => linear|exponential|none]` — re-run block until success or attempts exhausted.
    RetryBlock {
        body: Block,
        times: Box<Expr>,
        backoff: RetryBackoff,
    },
    /// `rate_limit(MAX, WINDOW) { BLOCK }` — sliding window: at most MAX runs per WINDOW (e.g. `"1s"`).
    /// `slot` is assigned at parse time for per-site state in the interpreter.
    RateLimitBlock {
        slot: u32,
        max: Box<Expr>,
        window: Box<Expr>,
        body: Block,
    },
    /// `every(INTERVAL) { BLOCK }` — repeat BLOCK forever with sleep (INTERVAL like `"5s"` or seconds).
    EveryBlock {
        interval: Box<Expr>,
        body: Block,
    },
    /// `gen { ... yield ... }` — lazy generator; call `->next` for each value.
    GenBlock {
        body: Block,
    },
    /// `yield EXPR` — only valid inside `gen { }` (and propagates through control flow).
    Yield(Box<Expr>),

    /// `match (EXPR) { PATTERN => EXPR, ... }` — first matching arm; bindings scoped to the arm body.
    AlgebraicMatch {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StringPart {
    Literal(String),
    ScalarVar(String),
    ArrayVar(String),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DerefKind {
    Array,
    Hash,
    Call,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    Negate,
    LogNot,
    BitNot,
    LogNotWord,
    PreIncrement,
    PreDecrement,
    Ref,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

    #[test]
    fn program_serializes_to_json() {
        let p = crate::parse("1+2;").expect("parse");
        let s = serde_json::to_string(&p).expect("json");
        assert!(s.contains("\"statements\""));
        assert!(s.contains("BinOp"));
    }
}
