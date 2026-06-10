//! AST node types for the Perl 5 interpreter.
//! Every node carries a `line` field for error reporting.

use serde::{Deserialize, Serialize};

fn default_delim() -> char {
    '/'
}
/// `Program` — see fields for layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    /// `statements` field.
    pub statements: Vec<Statement>,
}
/// `Statement` — see fields for layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Statement {
    /// Leading `LABEL:` on this statement (Perl convention: `FOO:`).
    pub label: Option<String>,
    /// `kind` field.
    pub kind: StmtKind,
    /// `line` field.
    pub line: usize,
}

impl Statement {
    /// `new` — see implementation.
    pub fn new(kind: StmtKind, line: usize) -> Self {
        Self {
            label: None,
            kind,
            line,
        }
    }
}

/// Surface spelling for `grep` / `greps` / `filter` (`fi`) / `find_all`.
/// `grep` is eager (Perl-compatible); `greps` / `filter` / `find_all` are lazy (streaming).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GrepBuiltinKeyword {
    /// `Grep` variant.
    #[default]
    Grep,
    /// `Greps` variant.
    Greps,
    /// `Filter` variant.
    Filter,
    /// `FindAll` variant.
    FindAll,
}

impl GrepBuiltinKeyword {
    /// `as_str` — see implementation.
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
    /// `$name`, `$name: Type`, or `$name = default` — one positional scalar from `@_`,
    /// optionally typed and/or with a default value.
    Scalar(String, Option<PerlTypeName>, Option<Box<Expr>>),
    /// `@name` or `@name = (default, list)` — slurps remaining positional args into an array.
    Array(String, Option<Box<Expr>>),
    /// `%name` or `%name = (key => val, ...)` — slurps remaining positional args into a hash.
    Hash(String, Option<Box<Expr>>),
    /// `[ $a, @tail, ... ]` — next argument must be array-like; same element rules as algebraic `match`.
    ArrayDestruct(Vec<MatchArrayElem>),
    /// `{ k => $v, ... }` — next argument must be a hash or hashref; keys bind to listed scalars.
    HashDestruct(Vec<(String, String)>),
}
/// `StmtKind` — see variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StmtKind {
    /// `Expression` variant.
    Expression(Expr),
    /// `If` variant.
    If {
        condition: Expr,
        body: Block,
        elsifs: Vec<(Expr, Block)>,
        else_block: Option<Block>,
    },
    /// `Unless` variant.
    Unless {
        condition: Expr,
        body: Block,
        else_block: Option<Block>,
    },
    /// `While` variant.
    While {
        condition: Expr,
        body: Block,
        label: Option<String>,
        /// `while (...) { } continue { }`
        continue_block: Option<Block>,
    },
    /// `Until` variant.
    Until {
        condition: Expr,
        body: Block,
        label: Option<String>,
        continue_block: Option<Block>,
    },
    /// `DoWhile` variant.
    DoWhile { body: Block, condition: Expr },
    /// `For` variant.
    For {
        init: Option<Box<Statement>>,
        condition: Option<Expr>,
        step: Option<Expr>,
        body: Block,
        label: Option<String>,
        continue_block: Option<Block>,
    },
    /// `Foreach` variant.
    Foreach {
        var: String,
        list: Expr,
        body: Block,
        label: Option<String>,
        continue_block: Option<Block>,
    },
    /// `SubDecl` variant.
    SubDecl {
        name: String,
        params: Vec<SubSigParam>,
        body: Block,
        /// Subroutine prototype text from `sub foo ($$) { }` (excluding parens).
        /// `None` when using structured [`SubSigParam`] signatures instead.
        prototype: Option<String>,
    },
    /// `Package` variant.
    Package { name: String },
    /// `Use` variant.
    Use { module: String, imports: Vec<Expr> },
    /// `use 5.008;` / `use 5;` — Perl version requirement (no-op at runtime in stryke).
    UsePerlVersion { version: f64 },
    /// `use overload '""' => 'as_string', '+' => 'add';` — operator maps (method names in current package).
    UseOverload { pairs: Vec<(String, String)> },
    /// `No` variant.
    No { module: String, imports: Vec<Expr> },
    /// `Return` variant.
    Return(Option<Expr>),
    /// `Last` variant.
    Last(Option<String>),
    /// `Next` variant.
    Next(Option<String>),
    /// `Redo` variant.
    Redo(Option<String>),
    /// `My` variant.
    My(Vec<VarDecl>),
    /// `Our` variant.
    Our(Vec<VarDecl>),
    /// `Local` variant.
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
    /// `oursync $x = 0` — package-global thread-safe atomic variable. Same as
    /// `mysync` but the binding lives in the package stash (e.g. `main::x`)
    /// so it is visible across packages and parallel workers share one cell.
    OurSync(Vec<VarDecl>),
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
    Goto { target: Box<Expr> },
    /// Standalone `continue { BLOCK }` (normally follows a loop; parsed for acceptance).
    Continue(Block),
    /// `struct Name { field => Type, ... }` — fixed-field records (`Name->new`, `$x->field`).
    StructDecl { def: StructDef },
    /// `enum Name { Variant1 => Type, Variant2, ... }` — algebraic data types.
    EnumDecl { def: EnumDef },
    /// `class Name extends Parent impl Trait { fields; methods }` — full OOP.
    ClassDecl { def: ClassDef },
    /// `trait Name { fn required; fn with_default { } }` — interface/mixin.
    TraitDecl { def: TraitDef },
    /// `eval_timeout SECS { ... }` — run block on a worker thread; main waits up to SECS (portable timeout).
    EvalTimeout { timeout: Expr, body: Block },
    /// `try { } catch ($err) { } [ finally { } ]` — catch runtime/die errors (not `last`/`next`/`return` flow).
    /// `finally` runs after a successful `try` or after `catch` completes (including if `catch` rethrows).
    TryCatch {
        try_block: Block,
        catch_var: String,
        catch_block: Block,
        finally_block: Option<Block>,
    },
    /// `given (EXPR) { when ... default ... }` — topic in `$_`, `when` matches with regex / eq / smartmatch.
    Given { topic: Expr, body: Block },
    /// `when (COND) { }` — only valid inside `given` (handled by given dispatcher).
    When { cond: Expr, body: Block },
    /// `default { }` — only valid inside `given`.
    DefaultCase { body: Block },
    /// `tie %hash` / `tie @arr` / `tie $x` — TIEHASH / TIEARRAY / TIESCALAR (FETCH/STORE).
    Tie {
        target: TieTarget,
        class: Expr,
        args: Vec<Expr>,
    },
    /// `format NAME =` picture/value lines … `.` — report templates for `write`.
    FormatDecl { name: String, lines: Vec<String> },
    /// `before|after|around "<glob>" { ... }` — register AOP advice on user subs.
    /// Pattern is a glob (`*`, `?`) matched against the called sub's bare name.
    AdviceDecl {
        kind: AdviceKind,
        pattern: String,
        body: Block,
    },
}

/// AOP advice kind for [`StmtKind::AdviceDecl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdviceKind {
    /// Run before the matched sub; sees `INTERCEPT_NAME` / `INTERCEPT_ARGS`.
    Before,
    /// Run after the matched sub; sees `INTERCEPT_MS` / `INTERCEPT_US` and the retval in `$?`.
    After,
    /// Wrap the matched sub; must call `proceed()` to invoke the original.
    Around,
}

/// Target of `tie` (hash, array, or scalar).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TieTarget {
    /// `Hash` variant.
    Hash(String),
    /// `Array` variant.
    Array(String),
    /// `Scalar` variant.
    Scalar(String),
}

/// Optional type for `typed my $x : Int` — enforced at assignment time (runtime).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerlTypeName {
    /// `Int` variant.
    Int,
    /// `Str` variant.
    Str,
    /// `Float` variant.
    Float,
    /// `Bool` variant.
    Bool,
    /// `Array` variant.
    Array,
    /// `Hash` variant.
    Hash,
    /// `Ref` variant.
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
    /// `name` field.
    pub name: String,
    /// `ty` field.
    pub ty: PerlTypeName,
    /// Optional default value expression (evaluated at construction time if field not provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// Method defined inside a struct: `fn name { ... }` or `fn name($self, ...) { ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructMethod {
    /// `name` field.
    pub name: String,
    /// `params` field.
    pub params: Vec<SubSigParam>,
    /// `body` field.
    pub body: Block,
}

/// Single variant in an enum definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    /// `name` field.
    pub name: String,
    /// Optional type for data carried by this variant. If None, it carries no data.
    pub ty: Option<PerlTypeName>,
}

/// Compile-time algebraic data type: `enum Name { Variant1 => Type, Variant2, ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    /// `name` field.
    pub name: String,
    /// `variants` field.
    pub variants: Vec<EnumVariant>,
}

impl EnumDef {
    /// `variant_index` — see implementation.
    #[inline]
    pub fn variant_index(&self, name: &str) -> Option<usize> {
        self.variants.iter().position(|v| v.name == name)
    }
    /// `variant` — see implementation.
    #[inline]
    pub fn variant(&self, name: &str) -> Option<&EnumVariant> {
        self.variants.iter().find(|v| v.name == name)
    }
}

/// Compile-time record type: `struct Name { field => Type, ... ; fn method { } }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    /// `name` field.
    pub name: String,
    /// `fields` field.
    pub fields: Vec<StructField>,
    /// User-defined methods: `fn name { }` inside struct body.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<StructMethod>,
}

/// Visibility modifier for class fields and methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Visibility {
    /// `Public` variant.
    #[default]
    Public,
    /// `Private` variant.
    Private,
    /// `Protected` variant.
    Protected,
}

/// Single field in a class definition: `name: Type = default` or `pub name: Type`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassField {
    /// `name` field.
    pub name: String,
    /// `ty` field.
    pub ty: PerlTypeName,
    /// `visibility` field.
    pub visibility: Visibility,
    /// `default` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// Method defined inside a class: `fn name { }` or `pub fn name($self, ...) { }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassMethod {
    /// `name` field.
    pub name: String,
    /// `params` field.
    pub params: Vec<SubSigParam>,
    /// `body` field.
    pub body: Option<Block>,
    /// `visibility` field.
    pub visibility: Visibility,
    /// `is_static` field.
    pub is_static: bool,
    /// `is_final` field.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_final: bool,
}

/// Trait definition: `trait Name { fn required; fn with_default { } }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitDef {
    /// `name` field.
    pub name: String,
    /// `methods` field.
    pub methods: Vec<ClassMethod>,
}

impl TraitDef {
    /// `method` — see implementation.
    #[inline]
    pub fn method(&self, name: &str) -> Option<&ClassMethod> {
        self.methods.iter().find(|m| m.name == name)
    }
    /// `required_methods` — see implementation.
    #[inline]
    pub fn required_methods(&self) -> impl Iterator<Item = &ClassMethod> {
        self.methods.iter().filter(|m| m.body.is_none())
    }
}

/// A static (class-level) variable: `static count: Int = 0`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassStaticField {
    /// `name` field.
    pub name: String,
    /// `ty` field.
    pub ty: PerlTypeName,
    /// `visibility` field.
    pub visibility: Visibility,
    /// `default` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Expr>,
}

/// Class definition: `class Name extends Parent impl Trait { fields; methods }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDef {
    /// `name` field.
    pub name: String,
    /// `is_abstract` field.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_abstract: bool,
    /// `is_final` field.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_final: bool,
    /// `extends` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extends: Vec<String>,
    /// `implements` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implements: Vec<String>,
    /// `fields` field.
    pub fields: Vec<ClassField>,
    /// `methods` field.
    pub methods: Vec<ClassMethod>,
    /// `static_fields` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_fields: Vec<ClassStaticField>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl ClassDef {
    /// `field_index` — see implementation.
    #[inline]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
    /// `field` — see implementation.
    #[inline]
    pub fn field(&self, name: &str) -> Option<&ClassField> {
        self.fields.iter().find(|f| f.name == name)
    }
    /// `method` — see implementation.
    #[inline]
    pub fn method(&self, name: &str) -> Option<&ClassMethod> {
        self.methods.iter().find(|m| m.name == name)
    }
    /// `static_methods` — see implementation.
    #[inline]
    pub fn static_methods(&self) -> impl Iterator<Item = &ClassMethod> {
        self.methods.iter().filter(|m| m.is_static)
    }
    /// `instance_methods` — see implementation.
    #[inline]
    pub fn instance_methods(&self) -> impl Iterator<Item = &ClassMethod> {
        self.methods.iter().filter(|m| !m.is_static)
    }
}

impl StructDef {
    /// `field_index` — see implementation.
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

    /// Strict runtime check: `Int` only integer-like [`StrykeValue`](crate::value::StrykeValue), `Str` only string, `Float` allows int or float.
    pub fn check_value(&self, v: &crate::value::StrykeValue) -> Result<(), String> {
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
                // Allow undef for struct/class types (nullable pattern)
                if v.is_undef() {
                    return Ok(());
                }
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
                } else if let Some(c) = v.as_class_inst() {
                    // Check class name and full inheritance hierarchy
                    if c.isa(name) {
                        Ok(())
                    } else {
                        Err(format!("expected {}, got {}", name, c.def.name))
                    }
                } else if let Some(b) = v.as_blessed_ref() {
                    // Old-style `bless {...}, "Class"` — accept as the
                    // nominal type if the class name matches. Lets typed-
                    // my survive any escape hatch that reaches the value
                    // through the Perl 5 OO path.
                    if b.class == *name {
                        Ok(())
                    } else {
                        Err(format!("expected {}, got {}", name, b.class))
                    }
                } else {
                    Err(format!("expected {}, got {}", name, v.type_name()))
                }
            }
            Self::Enum(name) => {
                // Allow undef for enum types (nullable pattern)
                if v.is_undef() {
                    return Ok(());
                }
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
/// `VarDecl` — see fields for layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarDecl {
    /// `sigil` field.
    pub sigil: Sigil,
    /// `name` field.
    pub name: String,
    /// `initializer` field.
    pub initializer: Option<Expr>,
    /// Set by `frozen my ...` — reassignments are rejected at compile time (bytecode) or runtime.
    pub frozen: bool,
    /// Set by `typed my $x : Int` (scalar only).
    pub type_annotation: Option<PerlTypeName>,
    /// True when declared with parens: `my ($x) = @a` vs `my $x = @a`.
    /// In list context, a scalar gets the first element; in scalar context, it gets the count.
    #[serde(default)]
    pub list_context: bool,
}
/// `Sigil` — see variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Sigil {
    /// `Scalar` variant.
    Scalar,
    /// `Array` variant.
    Array,
    /// `Hash` variant.
    Hash,
    /// `local *FH` — filehandle slot alias (limited typeglob).
    Typeglob,
}
/// `Block` type alias.
pub type Block = Vec<Statement>;

/// Comparator for `sort` — `{ $a <=> $b }`, or a code ref / expression (Perl `sort $cmp LIST`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortComparator {
    /// `Block` variant.
    Block(Block),
    /// `Code` variant.
    Code(Box<Expr>),
}

// ── Algebraic `match` expression (stryke extension) ──

/// One arm of [`ExprKind::AlgebraicMatch`]: `PATTERN [if EXPR] => EXPR`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchArm {
    /// `pattern` field.
    pub pattern: MatchPattern,
    /// Optional guard (`if EXPR`) evaluated after pattern match; `$_` is the match subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<Box<Expr>>,
    /// `body` field.
    pub body: Expr,
}

/// `retry { } backoff => exponential` — sleep policy between attempts (after failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryBackoff {
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
/// `MatchArrayElem` — see variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchArrayElem {
    /// `Expr` variant.
    Expr(Expr),
    /// `$name` at the top of a pattern element — bind this position to a new lexical `$name`.
    /// Use `[($x)]` if you need smartmatch against the current value of `$x` instead.
    CaptureScalar(String),
    /// Rest-of-array wildcard (only valid as the last element).
    Rest,
    /// `@name` — bind remaining elements as a new array to `@name` (only valid as the last element).
    RestBind(String),
}
/// `MatchHashPair` — see variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MatchHashPair {
    /// `key => _` — key must exist.
    KeyOnly { key: Expr },
    /// `key => $name` — key must exist; value is bound to `$name` in the arm.
    Capture { key: Expr, name: String },
}
/// `MagicConstKind` — see variants.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MagicConstKind {
    /// Current source path (`$0`-style script name or `-e`).
    File,
    /// Line number of this token (1-based, same as lexer).
    Line,
    /// Reference to currently executing subroutine (for anonymous recursion).
    Sub,
}
/// `Expr` — see fields for layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expr {
    /// `kind` field.
    pub kind: ExprKind,
    /// `line` field.
    pub line: usize,
}
/// `ExprKind` — see variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprKind {
    // Literals
    /// `Integer` variant.
    Integer(i64),
    /// `Float` variant.
    Float(f64),
    /// `String` variant.
    String(String),
    /// Unquoted identifier used as an expression term (`if (FOO)`), distinct from quoted `'FOO'` / `"FOO"`.
    /// Resolved at runtime: nullary subroutine if defined, otherwise stringifies like Perl barewords.
    Bareword(String),
    /// `Regex` variant.
    Regex(String, String),
    /// `QW` variant.
    QW(Vec<String>),
    /// `Undef` variant.
    Undef,
    /// `__FILE__` / `__LINE__` (Perl compile-time literals).
    MagicConst(MagicConstKind),

    // Interpolated string (mix of literal and variable parts)
    /// `InterpolatedString` variant.
    InterpolatedString(Vec<StringPart>),

    // Variables
    /// `ScalarVar` variant.
    ScalarVar(String),
    /// `ArrayVar` variant.
    ArrayVar(String),
    /// `HashVar` variant.
    HashVar(String),
    /// `ArrayElement` variant.
    ArrayElement {
        array: String,
        index: Box<Expr>,
    },
    /// `HashElement` variant.
    HashElement {
        hash: String,
        key: Box<Expr>,
    },
    /// `ArraySlice` variant.
    ArraySlice {
        array: String,
        indices: Vec<Expr>,
    },
    /// `HashSlice` variant.
    HashSlice {
        hash: String,
        keys: Vec<Expr>,
    },
    /// `%h{KEYS}` — Perl 5.20+ key-value slice: returns a flat list of
    /// (key, value, key, value, ...) pairs instead of just values. (BUG-008)
    HashKvSlice {
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
    /// `ScalarRef` variant.
    ScalarRef(Box<Expr>),
    /// `ArrayRef` variant.
    ArrayRef(Vec<Expr>),
    HashRef(Vec<(Expr, Expr)>),
    /// `CodeRef` variant.
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
    /// `Deref` variant.
    Deref {
        expr: Box<Expr>,
        kind: Sigil,
    },
    /// `ArrowDeref` variant.
    ArrowDeref {
        expr: Box<Expr>,
        index: Box<Expr>,
        kind: DerefKind,
    },

    // Operators
    /// `BinOp` variant.
    BinOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    /// `UnaryOp` variant.
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    /// `PostfixOp` variant.
    PostfixOp {
        expr: Box<Expr>,
        op: PostfixOp,
    },
    /// `Assign` variant.
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
    },
    /// `CompoundAssign` variant.
    CompoundAssign {
        target: Box<Expr>,
        op: BinOp,
        value: Box<Expr>,
    },
    /// `Ternary` variant.
    Ternary {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },

    // Repetition operator `EXPR x N`.
    //
    // Perl distinguishes scalar string repetition (`"ab" x 3` → `"ababab"`) from
    // list repetition (`(0) x 3` → `(0,0,0)`, `qw(a b) x 2` → `(a,b,a,b)`). The
    // discriminator at parse time is the LHS shape: a top-level paren-list (or
    // `qw(...)`) immediately before `x` is list-repeat; everything else is
    // scalar-repeat. The parser sets `list_repeat=true` only in that case;
    // `f(args) x N` (function-call parens, not list parens) stays scalar.
    /// `Repeat` variant.
    Repeat {
        expr: Box<Expr>,
        count: Box<Expr>,
        list_repeat: bool,
    },

    // Range: `1..10` / `1...10` — in scalar context, `...` is the exclusive flip-flop (Perl `sed`-style).
    // With step: `1..100:2` (1,3,5,...,99) or `100..1:-1` (100,99,...,1).
    /// `Range` variant.
    Range {
        from: Box<Expr>,
        to: Box<Expr>,
        #[serde(default)]
        exclusive: bool,
        #[serde(default)]
        step: Option<Box<Expr>>,
    },

    /// Slice subscript range with optional endpoints — Python-style `[start:stop:step]`.
    /// Only emitted by the parser inside `@arr[...]` / `@h{...}` (and arrow-deref forms).
    /// Open-ended forms: `[::-1]` (reverse), `[:N]`, `[N:]`, `[::M]`, `[N::M]`.
    /// Compiler dispatches to typed integer-strict (array) or stringify-all (hash) ops.
    SliceRange {
        #[serde(default)]
        from: Option<Box<Expr>>,
        #[serde(default)]
        to: Option<Box<Expr>>,
        #[serde(default)]
        step: Option<Box<Expr>>,
    },

    /// `my $x = EXPR` (or `our` / `state` / `local`) used as an *expression* —
    /// e.g. inside `if (my $line = readline)` / `while (my $x = next())`.
    /// Evaluation: declare each var in the current scope, evaluate the initializer
    /// (or default to `undef`), then return the assigned value(s).
    /// Distinct from `StmtKind::My` which only appears at statement level.
    ///
    /// `var $x = EXPR` and `val $x = EXPR` (Kotlin/Scala-style aliases for
    /// `my` and `const my`) reach this node via parser-level normalization
    /// — `parse_primary` rewrites `raw_kw` to `"my"` and threads the
    /// `mark_frozen` bit through `VarDecl::frozen` on each decl. The
    /// `keyword` field below therefore only ever holds the post-normalized
    /// spelling; `"var"`/`"val"` do not appear at the AST layer.
    MyExpr {
        keyword: String, // "my" / "our" / "state" / "local" (post-normalization; `var`/`val` collapse into `"my"`)
        decls: Vec<VarDecl>,
    },

    // Function call
    /// `FuncCall` variant.
    FuncCall {
        name: String,
        args: Vec<Expr>,
    },

    // Method call: $obj->method(args) or $obj->SUPER::method(args)
    /// `MethodCall` variant.
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
    /// `Print` variant.
    Print {
        handle: Option<String>,
        args: Vec<Expr>,
    },
    /// `Say` variant.
    Say {
        handle: Option<String>,
        args: Vec<Expr>,
    },
    /// `Printf` variant.
    Printf {
        handle: Option<String>,
        args: Vec<Expr>,
    },
    /// `Die` variant.
    Die(Vec<Expr>),
    /// `Warn` variant.
    Warn(Vec<Expr>),

    // Regex operations
    /// `Match` variant.
    Match {
        expr: Box<Expr>,
        pattern: String,
        flags: String,
        /// When true, `/g` uses Perl scalar semantics (one match per eval, updates `pos`).
        scalar_g: bool,
        #[serde(default = "default_delim")]
        delim: char,
    },
    /// `Substitution` variant.
    Substitution {
        expr: Box<Expr>,
        pattern: String,
        replacement: String,
        flags: String,
        #[serde(default = "default_delim")]
        delim: char,
    },
    /// `Transliterate` variant.
    Transliterate {
        expr: Box<Expr>,
        from: String,
        to: String,
        flags: String,
        #[serde(default = "default_delim")]
        delim: char,
    },

    // List operations
    /// `MapExpr` variant.
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
    /// `GrepExpr` variant.
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
    /// `ReverseExpr` variant.
    ReverseExpr(Box<Expr>),
    /// `rev EXPR` — always string-reverse (scalar reverse), stryke extension.
    Rev(Box<Expr>),
    /// `JoinExpr` variant.
    JoinExpr {
        separator: Box<Expr>,
        list: Box<Expr>,
    },
    /// `SplitExpr` variant.
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
    /// `PMapExpr` variant.
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
    /// `PGrepExpr` variant.
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
    /// `par { BLOCK } INPUT` — generic parallel-chunk wrapper. Splits INPUT
    /// (string → UTF-8-aligned byte chunks; array/list → element-chunks)
    /// into N pieces (N = available rayon threads), evaluates BLOCK per
    /// chunk in parallel with `$_` bound to the chunk, then concatenates
    /// results. Lets any whole-input op (`letters`, `chars`, `uc`, `freq`,
    /// regex `//g`, etc.) parallelize without needing a `pX` variant.
    ParExpr {
        block: Block,
        list: Box<Expr>,
    },
    /// `par_reduce { extract } [ { merge } ] INPUT` — chunk-extract-merge.
    /// Same chunker as `par {}`, but each chunk's result is reduced
    /// pairwise across chunks instead of concatenated.
    ///
    /// - One block: auto-merger picks based on result type (number → `+`,
    ///   `hash<num>` → key-wise `+`, array → concat, string → concat).
    /// - Two blocks: explicit pairwise reducer with `$a`/`$b`.
    ParReduceExpr {
        extract_block: Block,
        reduce_block: Option<Block>,
        list: Box<Expr>,
    },
    /// Distributed counterpart of [`ExprKind::ParReduceExpr`]. Same chunk-block
    /// semantics (stages operate on `@_`) but chunks ship to a `RemoteCluster`
    /// of SSH workers via the existing `cluster::run_cluster` dispatcher.
    /// Built by `~d> on $cluster SOURCE stage1 stage2 ...`.
    DistReduceExpr {
        cluster: Box<Expr>,
        extract_block: Block,
        list: Box<Expr>,
    },
    /// `par_lines PATH, fn { ... } [, progress => EXPR]` — optional stderr progress (per line).
    ParLinesExpr {
        path: Box<Expr>,
        callback: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `par_walk PATH, fn { ... } [, progress => EXPR]` — parallel recursive directory walk; `$_` is each path.
    ParWalkExpr {
        path: Box<Expr>,
        callback: Box<Expr>,
        progress: Option<Box<Expr>>,
    },
    /// `pwatch GLOB, fn { ... }` — notify-based watcher (evaluated by interpreter).
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
    /// `reduce { $a + $b } @list` — sequential left fold over the list.
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
    /// `swallow PATTERN` — expand a zsh-style glob and return a hash
    /// `{ canonicalized_abspath => raw_bytes }`. Per-file body never decodes,
    /// so binary files round-trip cleanly. Hard-fails on non-regular matches
    /// the same way `slurp` does; opt out with the `(N)` null-glob qualifier.
    Swallow(Box<Expr>),
    /// `burp HASH` — inverse of `swallow`. Take a hash `{ path => bytes }`,
    /// write each entry to disk (creates parent directories automatically),
    /// and return the number of files written. Hard-fails on the first I/O
    /// error. Accepts plain hashes and hash refs; values may be bytes or any
    /// scalar that stringifies (matches `spew`/`spurt` conventions).
    Burp(Box<Expr>),
    /// `god EXPR` — omniscient runtime introspection. Returns a structured
    /// multi-line dump showing the type tag, heap pointer, Arc strong/weak
    /// counts, byte hex previews, generator/pipeline state, and closure
    /// captures. Cycle-safe via per-pointer recursion tracking. Sibling to
    /// `pp` (human-friendly) and `ddump` (deep structure).
    God(Box<Expr>),
    /// `ingest PATTERN` — streaming variant of `swallow`: returns a lazy
    /// iterator yielding `[canonicalized_abspath, raw_bytes]` per file. Only
    /// one file's bytes are resident at a time. Path list and stat/canonicalize
    /// are eager (full zsh qualifier support); file reads are lazy. Hard-fails
    /// on non-regular matches up-front, matching `slurp`/`swallow` policy.
    Ingest(Box<Expr>),
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
    /// `Push` variant.
    Push {
        array: Box<Expr>,
        values: Vec<Expr>,
    },
    /// `Pop` variant.
    Pop(Box<Expr>),
    /// `Shift` variant.
    Shift(Box<Expr>),
    /// `Unshift` variant.
    Unshift {
        array: Box<Expr>,
        values: Vec<Expr>,
    },
    /// `Splice` variant.
    Splice {
        array: Box<Expr>,
        offset: Option<Box<Expr>>,
        length: Option<Box<Expr>>,
        replacement: Vec<Expr>,
    },
    /// `Delete` variant.
    Delete(Box<Expr>),
    /// `Exists` variant.
    Exists(Box<Expr>),
    /// `Keys` variant.
    Keys(Box<Expr>),
    /// `Values` variant.
    Values(Box<Expr>),
    /// `Each` variant.
    Each(Box<Expr>),

    // String operations
    /// `Chomp` variant.
    Chomp(Box<Expr>),
    /// `Chop` variant.
    Chop(Box<Expr>),
    /// `Length` variant.
    Length(Box<Expr>),
    /// `Substr` variant.
    Substr {
        string: Box<Expr>,
        offset: Box<Expr>,
        length: Option<Box<Expr>>,
        replacement: Option<Box<Expr>>,
    },
    /// `Index` variant.
    Index {
        string: Box<Expr>,
        substr: Box<Expr>,
        position: Option<Box<Expr>>,
    },
    /// `Rindex` variant.
    Rindex {
        string: Box<Expr>,
        substr: Box<Expr>,
        position: Option<Box<Expr>>,
    },
    /// `Sprintf` variant.
    Sprintf {
        format: Box<Expr>,
        args: Vec<Expr>,
    },

    // Numeric
    /// `Abs` variant.
    Abs(Box<Expr>),
    /// `Int` variant.
    Int(Box<Expr>),
    /// `Sqrt` variant.
    Sqrt(Box<Expr>),
    /// `Sin` variant.
    Sin(Box<Expr>),
    /// `Cos` variant.
    Cos(Box<Expr>),
    /// `Atan2` variant.
    Atan2 {
        y: Box<Expr>,
        x: Box<Expr>,
    },
    /// `Exp` variant.
    Exp(Box<Expr>),
    /// `Log` variant.
    Log(Box<Expr>),
    /// `rand` with optional upper bound (none = Perl default 1.0).
    Rand(Option<Box<Expr>>),
    /// `srand` with optional seed (none = time-based).
    Srand(Option<Box<Expr>>),
    /// `Hex` variant.
    Hex(Box<Expr>),
    /// `Oct` variant.
    Oct(Box<Expr>),

    // Case
    /// `Lc` variant.
    Lc(Box<Expr>),
    /// `Uc` variant.
    Uc(Box<Expr>),
    /// `Lcfirst` variant.
    Lcfirst(Box<Expr>),
    /// `Ucfirst` variant.
    Ucfirst(Box<Expr>),

    /// Unicode case fold (Perl `fc`).
    Fc(Box<Expr>),
    /// Regex-escape a string (Perl `quotemeta`, aliased `qm`). Lowers to
    /// `Op::CallBuiltin(BuiltinId::Quotemeta, 1)` for JIT lowering.
    Quotemeta(Box<Expr>),
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
    /// `Defined` variant.
    Defined(Box<Expr>),
    /// `Ref` variant.
    Ref(Box<Expr>),
    /// `ScalarContext` variant.
    ScalarContext(Box<Expr>),

    // Char
    /// `Chr` variant.
    Chr(Box<Expr>),
    /// `Ord` variant.
    Ord(Box<Expr>),

    // I/O
    /// `open my $fh` — only valid as [`ExprKind::Open::handle`]; declares `$fh` and binds the handle.
    OpenMyHandle {
        name: String,
    },
    /// `Open` variant.
    Open {
        handle: Box<Expr>,
        mode: Box<Expr>,
        file: Option<Box<Expr>>,
    },
    /// `Close` variant.
    Close(Box<Expr>),
    /// `ReadLine` variant.
    ReadLine(Option<String>),
    /// `Eof` variant.
    Eof(Option<Box<Expr>>),
    /// `opendir my $dh` — only valid as [`ExprKind::Opendir::handle`]; declares `$dh` and binds the handle.
    OpendirMyHandle {
        name: String,
    },
    /// `Opendir` variant.
    Opendir {
        handle: Box<Expr>,
        path: Box<Expr>,
    },
    /// `Readdir` variant.
    Readdir(Box<Expr>),
    /// `Closedir` variant.
    Closedir(Box<Expr>),
    /// `Rewinddir` variant.
    Rewinddir(Box<Expr>),
    /// `Telldir` variant.
    Telldir(Box<Expr>),
    /// `Seekdir` variant.
    Seekdir {
        handle: Box<Expr>,
        position: Box<Expr>,
    },

    // File tests
    /// `FileTest` variant.
    FileTest {
        op: char,
        expr: Box<Expr>,
    },

    // System
    /// `System` variant.
    System(Vec<Expr>),
    /// `Exec` variant.
    Exec(Vec<Expr>),
    /// `Eval` variant.
    Eval(Box<Expr>),
    /// `Do` variant.
    Do(Box<Expr>),
    /// `Require` variant.
    Require(Box<Expr>),
    /// `Exit` variant.
    Exit(Option<Box<Expr>>),
    /// `Chdir` variant.
    Chdir(Box<Expr>),
    /// `Mkdir` variant.
    Mkdir {
        path: Box<Expr>,
        mode: Option<Box<Expr>>,
    },
    /// `Unlink` variant.
    Unlink(Vec<Expr>),
    /// `Rename` variant.
    Rename {
        old: Box<Expr>,
        new: Box<Expr>,
    },
    /// `chmod MODE, @files` — first expr is mode, rest are paths.
    Chmod(Vec<Expr>),
    /// `chown UID, GID, @files` — first two are uid/gid, rest are paths.
    Chown(Vec<Expr>),
    /// `Stat` variant.
    Stat(Box<Expr>),
    /// `Lstat` variant.
    Lstat(Box<Expr>),
    /// `Link` variant.
    Link {
        old: Box<Expr>,
        new: Box<Expr>,
    },
    /// `Symlink` variant.
    Symlink {
        old: Box<Expr>,
        new: Box<Expr>,
    },
    /// `Readlink` variant.
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
    /// `exe` / `exe DIR` — list executable file names in a directory (default: `.`).
    Executables(Vec<Expr>),
    /// `Glob` variant.
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
    /// `Bless` variant.
    Bless {
        ref_expr: Box<Expr>,
        class: Option<Box<Expr>>,
    },

    // Caller
    /// `Caller` variant.
    Caller(Option<Box<Expr>>),

    // Wantarray
    /// `Wantarray` variant.
    Wantarray,

    // List / Context
    /// `List` variant.
    List(Vec<Expr>),

    // Postfix if/unless/while/until/for
    /// `PostfixIf` variant.
    PostfixIf {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    /// `PostfixUnless` variant.
    PostfixUnless {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    /// `PostfixWhile` variant.
    PostfixWhile {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    /// `PostfixUntil` variant.
    PostfixUntil {
        expr: Box<Expr>,
        condition: Box<Expr>,
    },
    /// `PostfixForeach` variant.
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
/// `StringPart` — see variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StringPart {
    /// `Literal` variant.
    Literal(String),
    /// `ScalarVar` variant.
    ScalarVar(String),
    /// `ArrayVar` variant.
    ArrayVar(String),
    /// `Expr` variant.
    Expr(Expr),
}
/// `DerefKind` — see variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DerefKind {
    /// `Array` variant.
    Array,
    /// `Hash` variant.
    Hash,
    /// `Call` variant.
    Call,
}
/// `BinOp` — see variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BinOp {
    /// `Add` variant.
    Add,
    /// `Sub` variant.
    Sub,
    /// `Mul` variant.
    Mul,
    /// `Div` variant.
    Div,
    /// `Mod` variant.
    Mod,
    /// `Pow` variant.
    Pow,
    /// `Concat` variant.
    Concat,
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
    /// `LogAnd` variant.
    LogAnd,
    /// `LogOr` variant.
    LogOr,
    /// `DefinedOr` variant.
    DefinedOr,
    /// `BitAnd` variant.
    BitAnd,
    /// `BitOr` variant.
    BitOr,
    /// `BitXor` variant.
    BitXor,
    /// `ShiftLeft` variant.
    ShiftLeft,
    /// `ShiftRight` variant.
    ShiftRight,
    /// `LogAndWord` variant.
    LogAndWord,
    /// `LogOrWord` variant.
    LogOrWord,
    /// `BindMatch` variant.
    BindMatch,
    /// `BindNotMatch` variant.
    BindNotMatch,
}
/// `UnaryOp` — see variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// `Negate` variant.
    Negate,
    /// `LogNot` variant.
    LogNot,
    /// `BitNot` variant.
    BitNot,
    /// `LogNotWord` variant.
    LogNotWord,
    /// `PreIncrement` variant.
    PreIncrement,
    /// `PreDecrement` variant.
    PreDecrement,
    /// `Ref` variant.
    Ref,
}
/// `PostfixOp` — see variants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PostfixOp {
    /// `Increment` variant.
    Increment,
    /// `Decrement` variant.
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

    // ─── GrepBuiltinKeyword ───────────────────────────────────────────

    #[test]
    fn grep_keyword_as_str_matrix() {
        assert_eq!(GrepBuiltinKeyword::Grep.as_str(), "grep");
        assert_eq!(GrepBuiltinKeyword::Greps.as_str(), "greps");
        assert_eq!(GrepBuiltinKeyword::Filter.as_str(), "filter");
        assert_eq!(GrepBuiltinKeyword::FindAll.as_str(), "find_all");
    }

    #[test]
    fn grep_keyword_is_stream_only_false_for_grep() {
        // `grep` is the collecting (non-streaming) variant; everything else streams.
        assert!(!GrepBuiltinKeyword::Grep.is_stream());
        assert!(GrepBuiltinKeyword::Greps.is_stream());
        assert!(GrepBuiltinKeyword::Filter.is_stream());
        assert!(GrepBuiltinKeyword::FindAll.is_stream());
    }

    // ─── PerlTypeName byte encoding ───────────────────────────────────

    #[test]
    fn perl_type_name_byte_roundtrip() {
        // 0..7 → simple types → back to same byte.
        for b in 0..=7u8 {
            let t = PerlTypeName::from_byte(b).unwrap_or_else(|| panic!("byte {b} unknown"));
            assert_eq!(t.as_byte(), Some(b), "round-trip failed for byte {b}");
        }
    }

    #[test]
    fn perl_type_name_unknown_bytes_return_none() {
        assert!(PerlTypeName::from_byte(8).is_none());
        assert!(PerlTypeName::from_byte(255).is_none());
    }

    #[test]
    fn perl_type_name_struct_and_enum_have_no_byte_encoding() {
        // Named types require name-pool lookup, not byte encoding.
        assert_eq!(PerlTypeName::Struct("Point".into()).as_byte(), None);
        assert_eq!(PerlTypeName::Enum("Color".into()).as_byte(), None);
    }

    #[test]
    fn perl_type_name_simple_byte_assignments_are_stable() {
        // Pin the byte ordering so VM bytecode doesn't shift accidentally.
        assert_eq!(PerlTypeName::Int.as_byte(), Some(0));
        assert_eq!(PerlTypeName::Str.as_byte(), Some(1));
        assert_eq!(PerlTypeName::Float.as_byte(), Some(2));
        assert_eq!(PerlTypeName::Bool.as_byte(), Some(3));
        assert_eq!(PerlTypeName::Array.as_byte(), Some(4));
        assert_eq!(PerlTypeName::Hash.as_byte(), Some(5));
        assert_eq!(PerlTypeName::Ref.as_byte(), Some(6));
        assert_eq!(PerlTypeName::Any.as_byte(), Some(7));
    }

    #[test]
    fn perl_type_name_display_name_simple_types() {
        assert_eq!(PerlTypeName::Int.display_name(), "Int");
        assert_eq!(PerlTypeName::Str.display_name(), "Str");
        assert_eq!(PerlTypeName::Float.display_name(), "Float");
        assert_eq!(PerlTypeName::Bool.display_name(), "Bool");
        assert_eq!(PerlTypeName::Array.display_name(), "Array");
        assert_eq!(PerlTypeName::Hash.display_name(), "Hash");
        assert_eq!(PerlTypeName::Ref.display_name(), "Ref");
        assert_eq!(PerlTypeName::Any.display_name(), "Any");
    }

    #[test]
    fn perl_type_name_display_name_named_types() {
        assert_eq!(PerlTypeName::Struct("Point".into()).display_name(), "Point");
        assert_eq!(PerlTypeName::Enum("Color".into()).display_name(), "Color");
    }

    // ─── PerlTypeName::check_value runtime type-check ─────────────────

    #[test]
    fn perl_type_int_accepts_integer_like() {
        let v = crate::value::StrykeValue::integer(42);
        assert!(PerlTypeName::Int.check_value(&v).is_ok());
    }

    #[test]
    fn perl_type_int_rejects_string() {
        let v = crate::value::StrykeValue::string("hi".into());
        let err = PerlTypeName::Int.check_value(&v);
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("Int"));
    }

    #[test]
    fn perl_type_str_accepts_string() {
        let v = crate::value::StrykeValue::string("hi".into());
        assert!(PerlTypeName::Str.check_value(&v).is_ok());
    }

    #[test]
    fn perl_type_float_accepts_both_int_and_float() {
        // Float is permissive — accepts integer-like too (numeric promotion).
        assert!(PerlTypeName::Float
            .check_value(&crate::value::StrykeValue::integer(7))
            .is_ok());
        assert!(PerlTypeName::Float
            .check_value(&crate::value::StrykeValue::float(3.14))
            .is_ok());
    }

    #[test]
    fn perl_type_bool_accepts_anything() {
        // Bool's check_value returns Ok(()) for everything (perl truthiness).
        assert!(PerlTypeName::Bool
            .check_value(&crate::value::StrykeValue::integer(0))
            .is_ok());
        assert!(PerlTypeName::Bool
            .check_value(&crate::value::StrykeValue::string("".into()))
            .is_ok());
        assert!(PerlTypeName::Bool
            .check_value(&crate::value::StrykeValue::UNDEF)
            .is_ok());
    }

    // ─── Statement::new constructor ───────────────────────────────────

    #[test]
    fn statement_new_preserves_line_and_kind() {
        let kind = StmtKind::Expression(Expr {
            kind: ExprKind::Integer(42),
            line: 7,
        });
        let s = Statement::new(kind, 7);
        assert_eq!(s.line, 7);
        // Round-trip the kind via debug formatting since pattern-match would
        // require StmtKind to be PartialEq.
        assert!(format!("{:?}", s.kind).contains("Expression"));
    }
}
