//! AST node types for the Perl 5 interpreter.
//! Every node carries a `line` field for error reporting.

use serde::{Deserialize, Serialize};

fn default_delim() -> char {
    '/'
}

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

/// Surface spelling for `grep` / `greps` / `filter` / `find_all`.
/// `grep` is eager (Perl-compatible); `greps` / `filter` / `find_all` are lazy (streaming).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GrepBuiltinKeyword {
    #[default]
    Grep,
    Greps,
    Filter,
    FindAll,
}

impl GrepBuiltinKeyword {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grep => "grep",
            Self::Greps => "greps",
            Self::Filter => "filter",
            Self::FindAll => "find_all",
        }
    }

    /// Returns `true` for streaming variants (`greps`, `filter`, `find_all`).
    pub const fn is_stream(self) -> bool {
        !matches!(self, Self::Grep)
    }
}

/// Named parameter in `sub name (SIG ...) { }` — stryke extension (not Perl 5 prototype syntax).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubSigParam {
    /// `$name` or `$name: Type` — one positional scalar from `@_`, optionally typed.
    Scalar(String, Option<PerlTypeName>),
    /// `@name` — slurps remaining positional args into an array.
    Array(String),
    /// `%name` — slurps remaining positional args into a hash (key-value pairs).
    Hash(String),
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
    /// `use 5.008;` / `use 5;` — Perl version requirement (no-op at runtime in stryke).
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
    /// `enum Name { Variant1 => Type, Variant2, ... }` — algebraic data types.
    EnumDecl {
        def: EnumDef,
    },
    /// `class Name extends Parent impl Trait { fields; methods }` — full OOP.
    ClassDecl {
        def: ClassDef,
    },
    /// `trait Name { fn required; fn with_default { } }` — interface/mixin.
    TraitDecl {
        def: TraitDef,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerlTypeName {
    Int,
    Str,
    Float,
    Bool,
    Array,
    Hash,
    Ref,
    /// Struct-typed field: `field => Point` where Point is a struct name.
    Struct(String),
    /// Enum-typed field: `field => Color` where Color is an enum name.
    Enum(String),
    /// Accepts any value (no runtime type check).
    Any,
}

/// Single field in a struct definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructField {
    pub name: String,
    pub ty: PerlTypeName,
    /// Optional default value expression (evaluated at construction time if field not provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// Method defined inside a struct: `fn name { ... }` or `fn name($self, ...) { ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructMethod {
    pub name: String,
    pub params: Vec<SubSigParam>,
    pub body: Block,
}

/// Single variant in an enum definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    /// Optional type for data carried by this variant. If None, it carries no data.
    pub ty: Option<PerlTypeName>,
}

/// Compile-time algebraic data type: `enum Name { Variant1 => Type, Variant2, ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

impl EnumDef {
    #[inline]
    pub fn variant_index(&self, name: &str) -> Option<usize> {
        self.variants.iter().position(|v| v.name == name)
    }

    #[inline]
    pub fn variant(&self, name: &str) -> Option<&EnumVariant> {
        self.variants.iter().find(|v| v.name == name)
    }
}

/// Compile-time record type: `struct Name { field => Type, ... ; fn method { } }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
    /// User-defined methods: `fn name { }` inside struct body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<StructMethod>,
}

/// Visibility modifier for class fields and methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

/// Single field in a class definition: `name: Type = default` or `pub name: Type`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassField {
    pub name: String,
    pub ty: PerlTypeName,
    pub visibility: Visibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// Method defined inside a class: `fn name { }` or `pub fn name($self, ...) { }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassMethod {
    pub name: String,
    pub params: Vec<SubSigParam>,
    pub body: Option<Block>,
    pub visibility: Visibility,
    pub is_static: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_final: bool,
}

/// Trait definition: `trait Name { fn required; fn with_default { } }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitDef {
    pub name: String,
    pub methods: Vec<ClassMethod>,
}

impl TraitDef {
    #[inline]
    pub fn method(&self, name: &str) -> Option<&ClassMethod> {
        self.methods.iter().find(|m| m.name == name)
    }

    #[inline]
    pub fn required_methods(&self) -> impl Iterator<Item = &ClassMethod> {
        self.methods.iter().filter(|m| m.body.is_none())
    }
}

/// A static (class-level) variable: `static count: Int = 0`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassStaticField {
    pub name: String,
    pub ty: PerlTypeName,
    pub visibility: Visibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// Class definition: `class Name extends Parent impl Trait { fields; methods }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_abstract: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_final: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<String>,
    pub fields: Vec<ClassField>,
    pub methods: Vec<ClassMethod>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_fields: Vec<ClassStaticField>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl ClassDef {
    #[inline]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    #[inline]
    pub fn field(&self, name: &str) -> Option<&ClassField> {
        self.fields.iter().find(|f| f.name == name)
    }

    #[inline]
    pub fn method(&self, name: &str) -> Option<&ClassMethod> {
        self.methods.iter().find(|m| m.name == name)
    }

    #[inline]
    pub fn static_methods(&self) -> impl Iterator<Item = &ClassMethod> {
        self.methods.iter().filter(|m| m.is_static)
    }

    #[inline]
    pub fn instance_methods(&self) -> impl Iterator<Item = &ClassMethod> {
        self.methods.iter().filter(|m| !m.is_static)
    }
}

impl StructDef {
    #[inline]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// Get field type by name.
    #[inline]
    pub fn field_type(&self, name: &str) -> Option<&PerlTypeName> {
        self.fields.iter().find(|f| f.name == name).map(|f| &f.ty)
    }

    /// Get method by name.
    #[inline]
    pub fn method(&self, name: &str) -> Option<&StructMethod> {
        self.methods.iter().find(|m| m.name == name)
    }
}

impl PerlTypeName {
    /// Bytecode encoding for `DeclareScalarTyped` / VM (only simple types; struct types use name pool).
    #[inline]
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Int),
            1 => Some(Self::Str),
            2 => Some(Self::Float),
            3 => Some(Self::Bool),
            4 => Some(Self::Array),
            5 => Some(Self::Hash),
            6 => Some(Self::Ref),
            7 => Some(Self::Any),
            _ => None,
        }
    }

    /// Bytecode encoding (simple types only; `Struct(name)` / `Enum(name)` requires separate name pool lookup).
    #[inline]
    pub fn as_byte(&self) -> Option<u8> {
        match self {
            Self::Int => Some(0),
            Self::Str => Some(1),
            Self::Float => Some(2),
            Self::Bool => Some(3),
            Self::Array => Some(4),
            Self::Hash => Some(5),
            Self::Ref => Some(6),
            Self::Any => Some(7),
            Self::Struct(_) | Self::Enum(_) => None,
        }
    }

    /// Display name for error messages.
    pub fn display_name(&self) -> String {
        match self {
            Self::Int => "Int".to_string(),
            Self::Str => "Str".to_string(),
            Self::Float => "Float".to_string(),
            Self::Bool => "Bool".to_string(),
            Self::Array => "Array".to_string(),
            Self::Hash => "Hash".to_string(),
            Self::Ref => "Ref".to_string(),
            Self::Any => "Any".to_string(),
            Self::Struct(name) => name.clone(),
            Self::Enum(name) => name.clone(),
        }
    }

    /// Strict runtime check: `Int` only integer-like [`PerlValue`](crate::value::PerlValue), `Str` only string, `Float` allows int or float.
    pub fn check_value(&self, v: &crate::value::PerlValue) -> Result<(), String> {
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
            Self::Bool => Ok(()),
            Self::Array => {
                if v.as_array_vec().is_some() || v.as_array_ref().is_some() {
                    Ok(())
                } else {
                    Err(format!("expected Array, got {}", v.type_name()))
                }
            }
            Self::Hash => {
                if v.as_hash_map().is_some() || v.as_hash_ref().is_some() {
                    Ok(())
                } else {
                    Err(format!("expected Hash, got {}", v.type_name()))
                }
            }
            Self::Ref => {
                if v.as_scalar_ref().is_some()
                    || v.as_array_ref().is_some()
                    || v.as_hash_ref().is_some()
                    || v.as_code_ref().is_some()
                {
                    Ok(())
                } else {
                    Err(format!("expected Ref, got {}", v.type_name()))
                }
            }
            Self::Struct(name) => {
                if let Some(s) = v.as_struct_inst() {
                    if s.def.name == *name {
                        Ok(())
                    } else {
                        Err(format!(
                            "expected struct {}, got struct {}",
                            name, s.def.name
                        ))
                    }
                } else if let Some(e) = v.as_enum_inst() {
                    if e.def.name == *name {
                        Ok(())
                    } else {
                        Err(format!("expected {}, got enum {}", name, e.def.name))
                    }
                } else {
                    Err(format!("expected {}, got {}", name, v.type_name()))
                }
            }
            Self::Enum(name) => {
                if let Some(e) = v.as_enum_inst() {
                    if e.def.name == *name {
                        Ok(())
                    } else {
                        Err(format!("expected enum {}, got enum {}", name, e.def.name))
                    }
                } else {
                    Err(format!("expected enum {}, got {}", name, v.type_name()))
                }
            }
            Self::Any => Ok(()),
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

// ── Algebraic `match` expression (stryke extension) ──

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
    /// Perl-truthy (stryke: `$gen->next` yields `[value, more]` with `more` truthy while iterating).
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
    /// Reference to currently executing subroutine (for anonymous recursion).
    Sub,
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

    /// `my $x = EXPR` (or `our` / `state` / `local`) used as an *expression* —
    /// e.g. inside `if (my $line = readline)` / `while (my $x = next())`.
    /// Evaluation: declare each var in the current scope, evaluate the initializer
    /// (or default to `undef`), then return the assigned value(s).
    /// Distinct from `StmtKind::My` which only appears at statement level.
    MyExpr {
        keyword: String, // "my" / "our" / "state" / "local"
        decls: Vec<VarDecl>,
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
        #[serde(default = "default_delim")]
        delim: char,
    },
    Substitution {
        expr: Box<Expr>,
        pattern: String,
        replacement: String,
        flags: String,
        #[serde(default = "default_delim")]
        delim: char,
    },
    Transliterate {
        expr: Box<Expr>,
        from: String,
        to: String,
        flags: String,
        #[serde(default = "default_delim")]
        delim: char,
    },

    // List operations
    MapExpr {
        block: Block,
        list: Box<Expr>,
        /// `flat_map { }` — peel one ARRAY ref from each iteration (stryke extension).
        flatten_array_refs: bool,
        /// `maps` / `flat_maps` — lazy iterator output (stryke); `map` / `flat_map` use `false`.
        #[serde(default)]
        stream: bool,
    },
    /// `map EXPR, LIST` — EXPR is evaluated in list context with `$_` set to each element.
    MapExprComma {
        expr: Box<Expr>,
        list: Box<Expr>,
        flatten_array_refs: bool,
        #[serde(default)]
        stream: bool,
    },
    GrepExpr {
        block: Block,
        list: Box<Expr>,
        #[serde(default)]
        keyword: GrepBuiltinKeyword,
    },
    /// `grep EXPR, LIST` — EXPR is evaluated with `$_` set to each element (Perl list vs scalar context).
    GrepExprComma {
        expr: Box<Expr>,
        list: Box<Expr>,
        #[serde(default)]
        keyword: GrepBuiltinKeyword,
    },
    /// `sort BLOCK LIST`, `sort SUB LIST`, or `sort $coderef LIST` (Perl uses `$a`/`$b` in the comparator).
    SortExpr {
        cmp: Option<SortComparator>,
        list: Box<Expr>,
    },
    ReverseExpr(Box<Expr>),
    /// `rev EXPR` — always string-reverse (scalar reverse), stryke extension.
    ScalarReverse(Box<Expr>),
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
        /// `pmap_on $cluster { } @list` — fan out over SSH (`stryke --remote-worker`); `None` = local rayon.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on_cluster: Option<Box<Expr>>,
        /// `pmaps` / `pflat_maps` — streaming variant: returns a lazy iterator that processes
        /// chunks in parallel via rayon instead of eagerly collecting all results.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stream: bool,
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
        /// `pgreps` — streaming variant: returns a lazy iterator.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        stream: bool,
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
    /// `spinner "msg" { BLOCK }` — animated spinner on stderr while block runs.
    Spinner {
        message: Box<Expr>,
        body: Block,
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
    /// `files` / `files DIR` — list file names in a directory (default: `.`).
    Files(Vec<Expr>),
    /// `filesf` / `filesf DIR` / `f` — list only regular file names in a directory (default: `.`).
    Filesf(Vec<Expr>),
    /// `fr DIR` — list only regular file names recursively (default: `.`).
    FilesfRecursive(Vec<Expr>),
    /// `dirs` / `dirs DIR` / `d` — list subdirectory names in a directory (default: `.`).
    Dirs(Vec<Expr>),
    /// `dr DIR` — list subdirectory paths recursively (default: `.`).
    DirsRecursive(Vec<Expr>),
    /// `sym_links` / `sym_links DIR` — list symlink names in a directory (default: `.`).
    SymLinks(Vec<Expr>),
    /// `sockets` / `sockets DIR` — list Unix socket names in a directory (default: `.`).
    Sockets(Vec<Expr>),
    /// `pipes` / `pipes DIR` — list named-pipe (FIFO) names in a directory (default: `.`).
    Pipes(Vec<Expr>),
    /// `block_devices` / `block_devices DIR` — list block device names in a directory (default: `.`).
    BlockDevices(Vec<Expr>),
    /// `char_devices` / `char_devices DIR` — list character device names in a directory (default: `.`).
    CharDevices(Vec<Expr>),
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
