use serde::{Deserialize, Serialize};

use crate::ast::{
    AdviceKind, Block, ClassDef, EnumDef, Expr, MatchArm, StructDef, SubSigParam, TraitDef,
};
use crate::value::PerlValue;

/// `splice` operand tuple: array expr, offset, length, replacement list (see [`Chunk::splice_expr_entries`]).
pub(crate) type SpliceExprEntry = (Expr, Option<Expr>, Option<Expr>, Vec<Expr>);

/// `sub` body registered at run time (e.g. `BEGIN { sub f { ... } }`), mirrored from
/// [`crate::interpreter::Interpreter::exec_statement`] `StmtKind::SubDecl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSubDecl {
    pub name: String,
    pub params: Vec<SubSigParam>,
    pub body: Block,
    pub prototype: Option<String>,
}

/// AOP advice registered at runtime (`before|after|around "<glob>" { ... }`).
/// Installed via [`Op::RegisterAdvice`] into `Interpreter::intercepts`.
///
/// `body_block_idx` indexes [`Chunk::blocks`]. The body is lowered to bytecode
/// during the fourth-pass block lowering ([`Chunk::block_bytecode_ranges`]) so
/// `dispatch_with_advice` can run it through the VM (`run_block_region`) — the
/// same path used by `map { }` / `grep { }` blocks. This keeps advice on the
/// bytecode dispatch surface, away from the AST tree-walker, so compile-time
/// name resolution (`our`-qualified scalars, lexical slots) works inside the
/// advice exactly as it does outside. See `tests/tree_walker_absent_aop.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAdviceDecl {
    pub kind: AdviceKind,
    pub pattern: String,
    pub body: Block,
    pub body_block_idx: u16,
}

/// Stack-based bytecode instruction set for the stryke VM.
/// Operands use u16 for pool indices (64k names/constants) and i32 for jumps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Op {
    Nop,
    // ── Constants ──
    LoadInt(i64),
    LoadFloat(f64),
    LoadConst(u16), // index into constant pool
    LoadUndef,

    // ── Stack ──
    Pop,
    Dup,
    /// Duplicate the top two stack values: \[a, b\] (b on top) → \[a, b, a, b\].
    Dup2,
    /// Swap the top two stack values (PerlValue).
    Swap,
    /// Rotate the top three values upward (FORTH `rot`): `[a, b, c]` (c on top) → `[b, c, a]`.
    Rot,
    /// Pop one value; push [`PerlValue::scalar_context`] of that value (Perl aggregate rules).
    ValueScalarContext,

    // ── Scalars (u16 = name pool index) ──
    GetScalar(u16),
    /// Like `GetScalar` but reads `scope.get_scalar` only (no Perl special-variable dispatch).
    GetScalarPlain(u16),
    SetScalar(u16),
    /// Like `SetScalar` but calls `scope.set_scalar` only (no special-variable dispatch).
    SetScalarPlain(u16),
    DeclareScalar(u16),
    /// Like `DeclareScalar` but the binding is immutable after initialization.
    DeclareScalarFrozen(u16),
    /// `typed my $x : Type` — u8 encodes [`crate::ast::PerlTypeName`] (0=Int,1=Str,2=Float).
    DeclareScalarTyped(u16, u8),
    /// `frozen typed my $x : Type` — immutable after initialization + type-checked.
    DeclareScalarTypedFrozen(u16, u8),

    // ── State variables (persist across calls) ──
    /// `state $x = EXPR` — pop TOS as initializer on first call only.
    /// On subsequent calls the persisted value is used as the local binding.
    /// Key: (sub entry IP, name_idx) in VM's state_vars table.
    DeclareStateScalar(u16),
    /// `state @arr = (...)` — array variant.
    DeclareStateArray(u16),
    /// `state %hash = (...)` — hash variant.
    DeclareStateHash(u16),

    // ── Arrays ──
    GetArray(u16),
    SetArray(u16),
    DeclareArray(u16),
    DeclareArrayFrozen(u16),
    GetArrayElem(u16), // stack: [index] → value
    SetArrayElem(u16), // stack: [value, index]
    /// Like [`Op::SetArrayElem`] but leaves the assigned value on the stack (e.g. `$a[$i] //=`).
    SetArrayElemKeep(u16),
    PushArray(u16),  // stack: [value] → push to named array
    PopArray(u16),   // → popped value
    ShiftArray(u16), // → shifted value
    ArrayLen(u16),   // → integer length
    /// Pop index spec (scalar or array from [`Op::Range`]); push one `PerlValue::array` of elements
    /// read from the named array. Used for `@name[...]` slice rvalues.
    ArraySlicePart(u16),
    /// Pop `b`, pop `a` (arrays); push concatenation `a` followed by `b` (Perl slice / list glue).
    ArrayConcatTwo,
    /// `exists $a[$i]` — stack: `[index]` → 0/1 (stash-qualified array name pool index).
    ExistsArrayElem(u16),
    /// `delete $a[$i]` — stack: `[index]` → deleted value (or undef).
    DeleteArrayElem(u16),

    // ── Hashes ──
    GetHash(u16),
    SetHash(u16),
    DeclareHash(u16),
    DeclareHashFrozen(u16),
    /// Dynamic `local $x` — save previous binding, assign TOS (same stack shape as DeclareScalar).
    LocalDeclareScalar(u16),
    LocalDeclareArray(u16),
    LocalDeclareHash(u16),
    /// `local $h{key} = val` — stack: `[value, key]` (key on top), same as [`Op::SetHashElem`].
    LocalDeclareHashElement(u16),
    /// `local $a[i] = val` — stack: `[value, index]` (index on top), same as [`Op::SetArrayElem`].
    LocalDeclareArrayElement(u16),
    /// `local *name` or `local *name = *other` — second pool index is `Some(rhs)` when aliasing.
    LocalDeclareTypeglob(u16, Option<u16>),
    /// `local *{EXPR}` / `local *$x` — LHS glob name string on stack (TOS); optional static `*rhs` pool index.
    LocalDeclareTypeglobDynamic(Option<u16>),
    GetHashElem(u16), // stack: [key] → value
    SetHashElem(u16), // stack: [value, key]
    /// Like [`Op::SetHashElem`] but leaves the assigned value on the stack (e.g. `$h{k} //=`).
    SetHashElemKeep(u16),
    DeleteHashElem(u16), // stack: [key] → deleted value
    ExistsHashElem(u16), // stack: [key] → 0/1
    /// `delete $href->{key}` — stack: `[container, key]` (key on top) → deleted value.
    DeleteArrowHashElem,
    /// `exists $href->{key}` — stack: `[container, key]` → 0/1.
    ExistsArrowHashElem,
    /// `exists $aref->[$i]` — stack: `[container, index]` (index on top, int-coerced).
    ExistsArrowArrayElem,
    /// `delete $aref->[$i]` — stack: `[container, index]` → deleted value (or undef).
    DeleteArrowArrayElem,
    HashKeys(u16),   // → array of keys
    HashValues(u16), // → array of values
    /// Scalar `keys %h` — push integer key count.
    HashKeysScalar(u16),
    /// Scalar `values %h` — push integer value count.
    HashValuesScalar(u16),
    /// `keys EXPR` after operand evaluated in list context — stack: `[value]` → key list array.
    KeysFromValue,
    /// Scalar `keys EXPR` after operand — stack: `[value]` → key count.
    KeysFromValueScalar,
    /// `values EXPR` after operand evaluated in list context — stack: `[value]` → values array.
    ValuesFromValue,
    /// Scalar `values EXPR` after operand — stack: `[value]` → value count.
    ValuesFromValueScalar,

    /// `push @$aref, ITEM` — stack: `[aref, item]` (item on top); mutates; pushes `aref` back.
    PushArrayDeref,
    /// After `push @$aref, …` — stack: `[aref]` → `[len]` (consumes aref).
    ArrayDerefLen,
    /// `pop @$aref` — stack: `[aref]` → popped value.
    PopArrayDeref,
    /// `shift @$aref` — stack: `[aref]` → shifted value.
    ShiftArrayDeref,
    /// `unshift @$aref, LIST` — stack `[aref, v1, …, vn]` (vn on top); `n` extra values.
    UnshiftArrayDeref(u8),
    /// `splice @$aref, off, len, LIST` — stack top: replacements, then `len`, `off`, `aref` (`len` may be undef).
    SpliceArrayDeref(u8),

    // ── Arithmetic ──
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Negate,
    /// `inc EXPR` — pop value, push value + 1 (integer if input is integer, else float).
    Inc,
    /// `dec EXPR` — pop value, push value - 1.
    Dec,

    // ── String ──
    Concat,
    /// Pop array (or value coerced with [`PerlValue::to_list`]), join element strings with
    /// [`Interpreter::list_separator`] (`$"`), push one string. Used for `@a` in `"` / `qq`.
    ArrayStringifyListSep,
    StringRepeat,
    /// Pop string, apply `\U` / `\L` / `\u` / `\l` / `\Q` / `\E` case escapes, push result.
    ProcessCaseEscapes,

    // ── Comparison (numeric) ──
    NumEq,
    NumNe,
    NumLt,
    NumGt,
    NumLe,
    NumGe,
    Spaceship,

    // ── Comparison (string) ──
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    StrLe,
    StrGe,
    StrCmp,

    // ── Logical / Bitwise ──
    LogNot,
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,

    // ── Control flow (absolute target addresses) ──
    Jump(usize),
    JumpIfTrue(usize),
    JumpIfFalse(usize),
    /// Jump if TOS is falsy WITHOUT popping (for short-circuit &&)
    JumpIfFalseKeep(usize),
    /// Jump if TOS is truthy WITHOUT popping (for short-circuit ||)
    JumpIfTrueKeep(usize),
    /// Jump if TOS is defined WITHOUT popping (for //)
    JumpIfDefinedKeep(usize),

    // ── Increment / Decrement ──
    PreInc(u16),
    PreDec(u16),
    PostInc(u16),
    PostDec(u16),
    /// Pre-increment on a frame slot entry (compiled `my $x` fast path).
    PreIncSlot(u8),
    PreDecSlot(u8),
    PostIncSlot(u8),
    PostDecSlot(u8),

    // ── Functions ──
    /// Call subroutine: name index, arg count, `WantarrayCtx` discriminant as `u8`
    Call(u16, u8, u8),
    /// Like [`Op::Call`] but with a compile-time-resolved entry: `sid` indexes [`Chunk::static_sub_calls`]
    /// (entry IP + stack-args); `name_idx` duplicates the stash pool index for closure restore / JIT
    /// (same as in the table; kept in the opcode so JIT does not need the side table).
    CallStaticSubId(u16, u16, u8, u8),
    Return,
    ReturnValue,
    /// End of a compiled `map` / `grep` / `sort` block body (empty block or last statement an expression).
    /// Pops the synthetic call frame from [`crate::vm::VM::run_block_region`] and unwinds the
    /// block-local scope (`scope_push_hook` per iteration, like [`crate::interpreter::Interpreter::exec_block`]);
    /// not subroutine `return` and not a closure capture.
    BlockReturnValue,
    /// At runtime statement position: capture current lexicals into [`crate::value::PerlSub::closure_env`]
    /// for a sub already registered in [`Interpreter::subs`] (see `prepare_program_top_level`).
    BindSubClosure(u16),

    // ── Scope ──
    PushFrame,
    PopFrame,

    // ── I/O ──
    /// `print [HANDLE] LIST` — `None` uses [`crate::interpreter::Interpreter::default_print_handle`].
    Print(Option<u16>, u8),
    Say(Option<u16>, u8),

    // ── Built-in function calls ──
    /// Calls a registered built-in: (builtin_id, arg_count)
    CallBuiltin(u16, u8),
    /// Save [`crate::interpreter::Interpreter::wantarray_kind`] and set from `u8`
    /// ([`crate::interpreter::WantarrayCtx::as_byte`]). Used for `splice` / similar where the
    /// dynamic context must match the expression's compile-time [`WantarrayCtx`] (e.g. `print splice…`).
    WantarrayPush(u8),
    /// Restore after [`Op::WantarrayPush`].
    WantarrayPop,

    // ── List / Range ──
    MakeArray(u16), // pop N values, push as Array
    /// `@$href{k1,k2}` — stack: `[container, key1, …, keyN]` (TOS = last key); pops `N+1` values; pushes array of slot values.
    HashSliceDeref(u16),
    /// `@$aref[i1,i2,...]` — stack: `[array_ref, spec1, …, specN]` (TOS = last spec); each spec is a
    /// scalar index or array of indices (list-context `..` / `qw`/list). Pops `N+1`; pushes elements.
    ArrowArraySlice(u16),
    /// `@$href{k1,k2} = VALUE` — stack: `[value, container, key1, …, keyN]` (TOS = last key); pops `N+2` values.
    SetHashSliceDeref(u16),
    /// `%name{k1,k2} = VALUE` — stack: `[value, key1, …, keyN]` (TOS = last key); pops `N+1`. Pool: hash name, key count.
    SetHashSlice(u16, u16),
    /// `@h{k1,k2}` read — stack: `[key1, …, keyN]` (TOS = last key); pops `N` values; pushes array of slot values.
    /// Each key value may be a scalar or array (from list-context range); arrays are flattened into individual keys.
    /// Pool: hash name index, key-expression count.
    GetHashSlice(u16, u16),
    /// `@$href{k1,k2} OP= VALUE` — stack: `[rhs, container, key1, …, keyN]` (TOS = last key); pops `N+2`, pushes the new value.
    /// `u8` = [`crate::compiler::scalar_compound_op_to_byte`] encoding of the binop.
    /// Perl 5 applies the op only to the **last** key’s element.
    HashSliceDerefCompound(u8, u16),
    /// `++@$href{k1,k2}` / `--...` / `@$href{k1,k2}++` / `...--` — stack: `[container, key1, …, keyN]`;
    /// pops `N+1`. Pre-forms push the new last-element value; post-forms push the **old** last value.
    /// `u8` encodes kind: 0=PreInc, 1=PreDec, 2=PostInc, 3=PostDec. Only the last key is updated.
    HashSliceDerefIncDec(u8, u16),
    /// `@name{k1,k2} OP= rhs` — stack: `[rhs, key1, …, keyN]` (TOS = last key); pops `N+1`, pushes the new value.
    /// Pool: compound-op byte ([`crate::compiler::scalar_compound_op_to_byte`]), stash hash name, key-slot count.
    /// Only the **last** flattened key is updated (same as [`Op::HashSliceDerefCompound`]).
    NamedHashSliceCompound(u8, u16, u16),
    /// `++@name{k1,k2}` / `--…` / `@name{k1,k2}++` / `…--` — stack: `[key1, …, keyN]`; pops `N`.
    /// `u8` kind matches [`Op::HashSliceDerefIncDec`]. Only the last key is updated.
    NamedHashSliceIncDec(u8, u16, u16),
    /// Multi-key `@h{k1,k2} //=` / `||=` / `&&=` — stack `[key1, …, keyN]` unchanged; pushes the **last**
    /// flattened slot (Perl only tests that slot). Pool: hash name, key-slot count.
    NamedHashSlicePeekLast(u16, u16),
    /// Stack `[key1, …, keyN, cur]` — pop `N` key slots, keep `cur` (short-circuit path).
    NamedHashSliceDropKeysKeepCur(u16),
    /// Assign list RHS’s last element to the **last** flattened key; stack `[val, key1, …, keyN]` (TOS = last key). Pushes `val`.
    SetNamedHashSliceLastKeep(u16, u16),
    /// Multi-key `@$href{k1,k2} //=` — stack `[container, key1, …, keyN]`; pushes last slice element (see [`Op::ArrowArraySlicePeekLast`]).
    HashSliceDerefPeekLast(u16),
    /// `[container, key1, …, keyN, val]` → `[val, container, key1, …, keyN]` for [`Op::HashSliceDerefSetLastKeep`].
    HashSliceDerefRollValUnderKeys(u16),
    /// Assign to last flattened key only; stack `[val, container, key1, …, keyN]`. Pushes `val`.
    HashSliceDerefSetLastKeep(u16),
    /// Stack `[container, key1, …, keyN, cur]` — drop container and keys; keep `cur`.
    HashSliceDerefDropKeysKeepCur(u16),
    /// `@$aref[i1,i2,...] = LIST` — stack: `[value, aref, spec1, …, specN]` (TOS = last spec);
    /// pops `N+2`. Delegates to [`crate::interpreter::Interpreter::assign_arrow_array_slice`].
    SetArrowArraySlice(u16),
    /// `@$aref[i1,i2,...] OP= rhs` — stack: `[rhs, aref, spec1, …, specN]`; pops `N+2`, pushes new value.
    /// `u8` = [`crate::compiler::scalar_compound_op_to_byte`] encoding of the binop.
    /// Perl 5 applies the op only to the **last** index. Delegates to [`crate::interpreter::Interpreter::compound_assign_arrow_array_slice`].
    ArrowArraySliceCompound(u8, u16),
    /// `++@$aref[i1,i2,...]` / `--...` / `...++` / `...--` — stack: `[aref, spec1, …, specN]`;
    /// pops `N+1`. Pre-forms push the new last-element value; post-forms push the old last value.
    /// `u8` kind matches [`Op::HashSliceDerefIncDec`]. Only the last index is updated. Delegates to
    /// [`crate::interpreter::Interpreter::arrow_array_slice_inc_dec`].
    ArrowArraySliceIncDec(u8, u16),
    /// Read the element at the **last** flattened index of `@$aref[spec1,…]` without popping `aref`
    /// or specs. Stack: `[aref, spec1, …, specN]` (TOS = last spec) → same plus pushed scalar.
    /// Used for `@$r[i,j] //=` / `||=` / `&&=` short-circuit tests (Perl only tests the last slot).
    ArrowArraySlicePeekLast(u16),
    /// Stack: `[aref, spec1, …, specN, cur]` — pop slice keys and container, keep `cur` (short-circuit
    /// result). `u16` = number of spec slots (same as [`Op::ArrowArraySlice`]).
    ArrowArraySliceDropKeysKeepCur(u16),
    /// Reorder `[aref, spec1, …, specN, val]` → `[val, aref, spec1, …, specN]` for
    /// [`Op::SetArrowArraySliceLastKeep`].
    ArrowArraySliceRollValUnderSpecs(u16),
    /// Assign `val` to the **last** flattened index only; stack `[val, aref, spec1, …, specN]`
    /// (TOS = last spec). Pushes `val` (like [`Op::SetArrowArrayKeep`]).
    SetArrowArraySliceLastKeep(u16),
    /// Like [`Op::ArrowArraySliceIncDec`] but for a **named** stash array (`@a[i1,i2,...]`).
    /// Stack: `[spec1, …, specN]` (TOS = last spec). `u16` = name pool index (stash-qualified).
    /// Delegates to [`crate::interpreter::Interpreter::named_array_slice_inc_dec`].
    NamedArraySliceIncDec(u8, u16, u16),
    /// `@name[spec1,…] OP= rhs` — stack `[rhs, spec1, …, specN]` (TOS = last spec); pops `N+1`.
    /// Only the **last** flattened index is updated (same as [`Op::ArrowArraySliceCompound`]).
    NamedArraySliceCompound(u8, u16, u16),
    /// Read the **last** flattened slot of `@name[spec1,…]` without popping specs. Stack:
    /// `[spec1, …, specN]` → same plus pushed scalar. `u16` pairs: name pool index, spec count.
    NamedArraySlicePeekLast(u16, u16),
    /// Stack: `[spec1, …, specN, cur]` — pop specs, keep `cur` (short-circuit). `u16` = spec count.
    NamedArraySliceDropKeysKeepCur(u16),
    /// `[spec1, …, specN, val]` → `[val, spec1, …, specN]` for [`Op::SetNamedArraySliceLastKeep`].
    NamedArraySliceRollValUnderSpecs(u16),
    /// Assign to the **last** index only; stack `[val, spec1, …, specN]`. Pushes `val`.
    SetNamedArraySliceLastKeep(u16, u16),
    /// `@name[spec1,…] = LIST` — stack `[value, spec1, …, specN]` (TOS = last spec); pops `N+1`.
    /// Element-wise like [`Op::SetArrowArraySlice`]. Pool indices: stash-qualified array name, spec count.
    SetNamedArraySlice(u16, u16),
    /// `BAREWORD` as an rvalue — at run time, look up a subroutine with this name; if found,
    /// call it with no args (nullary), otherwise push the name as a string (Perl's bareword-as-
    /// stringifies behavior). `u16` is a name-pool index. Delegates to
    /// [`crate::interpreter::Interpreter::resolve_bareword_rvalue`].
    BarewordRvalue(u16),
    /// Throw `PerlError::runtime` with the message at constant pool index `u16`. Used by the compiler
    /// to hard-reject constructs whose only valid response is a runtime error
    /// (e.g. `++@$r`, `%{...}--`) without AST fallback.
    RuntimeErrorConst(u16),
    MakeHash(u16), // pop N key-value pairs, push as Hash
    Range,         // stack: [from, to] → Array
    RangeStep,     // stack: [from, to, step] → Array (stepped range)
    /// Array slice via colon range — `@arr[FROM:TO:STEP]` / `@arr[::-1]`.
    /// Stack: `[from, to, step]` — each may be `Undef` to mean "omitted" (uses array bounds).
    /// `u16` is the array name pool index. Endpoints must coerce to integer cleanly; otherwise
    /// runtime aborts (`die "slice: non-integer endpoint in array slice"`). Pushes the sliced array.
    ArraySliceRange(u16),
    /// Hash slice via colon range — `@h{FROM:TO:STEP}` (keys auto-quote like fat comma `=>`).
    /// Stack: `[from, to, step]` — open ends die (no notion of "all keys" in unordered hash).
    /// Endpoints stringify to hash keys; expansion uses numeric or magic-string-increment
    /// depending on whether both ends parse as numbers. `u16` is the hash name pool index.
    /// Pushes the array of slot values for the expanded keys.
    HashSliceRange(u16),
    /// Scalar `..` / `...` flip-flop (numeric bounds vs `$.` — [`Interpreter::scalar_flipflop_dot_line`]).
    /// Stack: `[from, to]` (ints); pushes `1` or `0`. `u16` indexes flip-flop slots; `u8` is `1` for `...`
    /// (exclusive: right bound only after `$.` is strictly past the line where the left bound matched).
    ScalarFlipFlop(u16, u8),
    /// Regex `..` / `...` flip-flop: both bounds are pattern literals; tests use `$_` and `$.` like Perl
    /// (`Interpreter::regex_flip_flop_eval`). Operand order: `slot`, `exclusive`, left pattern, left flags,
    /// right pattern, right flags (constant pool indices). No stack operands; pushes `0`/`1`.
    RegexFlipFlop(u16, u8, u16, u16, u16, u16),
    /// Regex `..` / `...` flip-flop with `eof` as the right operand (no arguments). Left bound matches `$_`;
    /// right bound is [`Interpreter::eof_without_arg_is_true`] (Perl `eof` in `-n`/`-p`). Operand order:
    /// `slot`, `exclusive`, left pattern, left flags.
    RegexEofFlipFlop(u16, u8, u16, u16),
    /// Regex `..` / `...` with a non-literal right operand (e.g. `m/a/ ... (m/b/ or m/c/)`). Left bound is
    /// pattern + flags; right is evaluated in boolean context each line (pool index into
    /// [`Chunk::regex_flip_flop_rhs_expr_entries`] / bytecode ranges). Operand order: `slot`, `exclusive`,
    /// left pattern, left flags, rhs expr index.
    RegexFlipFlopExprRhs(u16, u8, u16, u16, u16),
    /// Regex `..` / `...` with a numeric right operand (Perl: right bound is [`Interpreter::scalar_flipflop_dot_line`]
    /// vs literal line). Constant pool index holds the RHS line as [`PerlValue::integer`]. Operand order:
    /// `slot`, `exclusive`, left pattern, left flags, rhs line constant index.
    RegexFlipFlopDotLineRhs(u16, u8, u16, u16, u16),

    // ── Regex ──
    /// Match: pattern_const_idx, flags_const_idx, scalar_g, pos_key_name_idx (`u16::MAX` = `$_`);
    /// stack: string operand → result
    RegexMatch(u16, u16, bool, u16),
    /// Substitution `s///`: pattern, replacement, flags constant indices; lvalue index into chunk.
    /// stack: string (subject from LHS expr) → replacement count
    RegexSubst(u16, u16, u16, u16),
    /// Transliterate `tr///`: from, to, flags constant indices; lvalue index into chunk.
    /// stack: string → transliteration count
    RegexTransliterate(u16, u16, u16, u16),
    /// Dynamic `=~` / `!~`: pattern from RHS, subject from LHS; empty flags.
    /// stack: `[subject, pattern]` (pattern on top) → 0/1; `true` = negate (`!~`).
    RegexMatchDyn(bool),
    /// Regex literal as a value (`qr/PAT/FLAGS`) — pattern and flags string pool indices.
    LoadRegex(u16, u16),
    /// After [`RegexMatchDyn`] for bare `m//` in `&&` / `||`: pop 0/1; push `""` or `1` (Perl scalar).
    RegexBoolToScalar,
    /// `pos $var = EXPR` / `pos = EXPR` (implicit `$_`). Stack: `[value, key]` (key string on top).
    SetRegexPos,

    // ── Assign helpers ──
    /// SetScalar that also leaves the value on the stack (for chained assignment)
    SetScalarKeep(u16),
    /// `SetScalarKeep` for non-special scalars (see `SetScalarPlain`).
    SetScalarKeepPlain(u16),

    // ── Block-based operations (u16 = index into chunk.blocks) ──
    /// map { BLOCK } @list — block_idx; stack: \[list\] → \[mapped\]
    MapWithBlock(u16),
    /// flat_map { BLOCK } @list — like [`Op::MapWithBlock`] but peels one ARRAY ref per iteration ([`PerlValue::map_flatten_outputs`])
    FlatMapWithBlock(u16),
    /// grep { BLOCK } @list — block_idx; stack: \[list\] → \[filtered\]
    GrepWithBlock(u16),
    /// each { BLOCK } @list — block_idx; stack: \[list\] → \[count\]
    ForEachWithBlock(u16),
    /// map EXPR, LIST — index into [`Chunk::map_expr_entries`] / [`Chunk::map_expr_bytecode_ranges`];
    /// stack: \[list\] → \[mapped\]
    MapWithExpr(u16),
    /// flat_map EXPR, LIST — same pools as [`Op::MapWithExpr`]; stack: \[list\] → \[mapped\]
    FlatMapWithExpr(u16),
    /// grep EXPR, LIST — index into [`Chunk::grep_expr_entries`] / [`Chunk::grep_expr_bytecode_ranges`];
    /// stack: \[list\] → \[filtered\]
    GrepWithExpr(u16),
    /// `group_by { BLOCK } LIST` / `chunk_by { BLOCK } LIST` — consecutive runs where the block’s
    /// return value stringifies the same as the previous (`str_eq`); stack: \[list\] → \[arrayrefs\]
    ChunkByWithBlock(u16),
    /// `group_by EXPR, LIST` / `chunk_by EXPR, LIST` — same as [`Op::ChunkByWithBlock`] but key from
    /// `EXPR` with `$_` set each iteration; uses [`Chunk::map_expr_entries`].
    ChunkByWithExpr(u16),
    /// sort { BLOCK } @list — block_idx; stack: \[list\] → \[sorted\]
    SortWithBlock(u16),
    /// sort @list (no block) — stack: \[list\] → \[sorted\]
    SortNoBlock,
    /// sort $coderef LIST — stack: \[list, coderef\] (coderef on top); `u8` = wantarray for comparator calls.
    SortWithCodeComparator(u8),
    /// `{ $a <=> $b }` (0), `{ $a cmp $b }` (1), `{ $b <=> $a }` (2), `{ $b cmp $a }` (3)
    SortWithBlockFast(u8),
    /// `map { $_ * k }` with integer `k` — stack: \[list\] → \[mapped\]
    MapIntMul(i64),
    /// `grep { $_ % m == r }` with integer `m` (non-zero), `r` — stack: \[list\] → \[filtered\]
    GrepIntModEq(i64, i64),
    /// Parallel sort, same fast modes as [`Op::SortWithBlockFast`].
    PSortWithBlockFast(u8),
    /// `read(FH, $buf, LEN [, OFFSET])` — reads into a named variable.
    /// Stack: [filehandle, length] (offset optional via `ReadIntoVarOffset`).
    /// Writes result into `$name[u16]`, pushes bytes-read count (or undef on error).
    ReadIntoVar(u16),
    /// `chomp` on assignable expr: stack has value → chomped count; uses `chunk.lvalues[idx]`.
    ChompInPlace(u16),
    /// `chop` on assignable expr: stack has value → chopped char; uses `chunk.lvalues[idx]`.
    ChopInPlace(u16),
    /// Four-arg `substr LHS, OFF, LEN, REPL` — index into [`Chunk::substr_four_arg_entries`]; stack: \[\] → extracted slice string
    SubstrFourArg(u16),
    /// `keys EXPR` when `EXPR` is not a bare `%h` — [`Chunk::keys_expr_entries`] /
    /// [`Chunk::keys_expr_bytecode_ranges`]
    KeysExpr(u16),
    /// `values EXPR` when not a bare `%h` — [`Chunk::values_expr_entries`] /
    /// [`Chunk::values_expr_bytecode_ranges`]
    ValuesExpr(u16),
    /// Scalar `keys EXPR` (dynamic) — same pools as [`Op::KeysExpr`].
    KeysExprScalar(u16),
    /// Scalar `values EXPR` — same pools as [`Op::ValuesExpr`].
    ValuesExprScalar(u16),
    /// `delete EXPR` when not a fast `%h{...}` — index into [`Chunk::delete_expr_entries`]
    DeleteExpr(u16),
    /// `exists EXPR` when not a fast `%h{...}` — index into [`Chunk::exists_expr_entries`]
    ExistsExpr(u16),
    /// `push EXPR, ...` when not a bare `@name` — [`Chunk::push_expr_entries`]
    PushExpr(u16),
    /// `pop EXPR` when not a bare `@name` — [`Chunk::pop_expr_entries`]
    PopExpr(u16),
    /// `shift EXPR` when not a bare `@name` — [`Chunk::shift_expr_entries`]
    ShiftExpr(u16),
    /// `unshift EXPR, ...` when not a bare `@name` — [`Chunk::unshift_expr_entries`]
    UnshiftExpr(u16),
    /// `splice EXPR, ...` when not a bare `@name` — [`Chunk::splice_expr_entries`]
    SpliceExpr(u16),
    /// `$var .= expr` — append to scalar string in-place without cloning.
    /// Stack: \[value_to_append\] → \[resulting_string\]. u16 = name pool index of target scalar.
    ConcatAppend(u16),
    /// Slot-indexed `$var .= expr` — avoids frame walking and string comparison.
    /// Stack: \[value_to_append\] → \[resulting_string\]. u8 = slot index.
    ConcatAppendSlot(u8),
    /// Fused `$slot_a += $slot_b` — no stack traffic. Pushes result.
    AddAssignSlotSlot(u8, u8),
    /// Fused `$slot_a -= $slot_b` — no stack traffic. Pushes result.
    SubAssignSlotSlot(u8, u8),
    /// Fused `$slot_a *= $slot_b` — no stack traffic. Pushes result.
    MulAssignSlotSlot(u8, u8),
    /// Fused `if ($slot < INT) goto target` — replaces GetScalarSlot + LoadInt + NumLt + JumpIfFalse.
    /// (slot, i32_limit, jump_target)
    SlotLtIntJumpIfFalse(u8, i32, usize),
    /// Void-context `$slot_a += $slot_b` — no stack push. Replaces AddAssignSlotSlot + Pop.
    AddAssignSlotSlotVoid(u8, u8),
    /// Void-context `++$slot` — no stack push. Replaces PreIncSlot + Pop.
    PreIncSlotVoid(u8),
    /// Void-context `$slot .= expr` — no stack push. Replaces ConcatAppendSlot + Pop.
    ConcatAppendSlotVoid(u8),
    /// Fused loop backedge: `$slot += 1; if $slot < limit jump body_target; else fall through`.
    ///
    /// Replaces the trailing `PreIncSlotVoid(s) + Jump(top)` of a C-style `for (my $i=0; $i<N; $i=$i+1)`
    /// loop whose top op is a `SlotLtIntJumpIfFalse(s, limit, exit)`. The initial iteration still
    /// goes through the top check; this op handles all subsequent iterations in a single dispatch,
    /// halving the number of ops per loop trip for the `bench_loop`/`bench_string`/`bench_array` shape.
    /// (slot, i32_limit, body_target)
    SlotIncLtIntJumpBack(u8, i32, usize),
    /// Fused accumulator loop: `while $i < limit { $sum += $i; $i += 1 }` — runs the entire
    /// remaining counted-sum loop in native Rust, eliminating op dispatch per iteration.
    ///
    /// Fused when a `for (my $i = a; $i < N; $i = $i + 1) { $sum += $i }` body compiles down to
    /// exactly `AddAssignSlotSlotVoid(sum, i) + SlotIncLtIntJumpBack(i, limit, body_target)` with
    /// `body_target` pointing at the AddAssign — i.e. the body is 1 Perl statement. Both slots are
    /// left as integers on exit (same coercion as `AddAssignSlotSlotVoid` + `PreIncSlotVoid`).
    /// (sum_slot, i_slot, i32_limit)
    AccumSumLoop(u8, u8, i32),
    /// Fused string-append counted loop: `while $i < limit { $s .= CONST; $i += 1 }` — extends
    /// the `String` buffer in place once and pushes the literal `(limit - i)` times in a tight
    /// Rust loop, with `Arc::get_mut` → `reserve` → `push_str`. Falls back to the regular op
    /// sequence if the slot is not a uniquely-owned heap `String`.
    ///
    /// Fused when the loop body is exactly `LoadConst(c) + ConcatAppendSlotVoid(s) +
    /// SlotIncLtIntJumpBack(i, limit, body_target)` with `body_target` pointing at the `LoadConst`.
    /// (const_idx, s_slot, i_slot, i32_limit)
    ConcatConstSlotLoop(u16, u8, u8, i32),
    /// Fused array-push counted loop: `while $i < limit { push @a, $i; $i += 1 }` — reserves the
    /// target `Vec` once and pushes `PerlValue::integer(i)` in a tight Rust loop. Emitted when
    /// the loop body is exactly `GetScalarSlot(i) + PushArray(arr) + ArrayLen(arr) + Pop +
    /// SlotIncLtIntJumpBack(i, limit, body_target)` with `body_target` pointing at the
    /// `GetScalarSlot` (i.e. the body is one `push` statement whose return is discarded).
    /// (arr_name_idx, i_slot, i32_limit)
    PushIntRangeToArrayLoop(u16, u8, i32),
    /// Fused hash-insert counted loop: `while $i < limit { $h{$i} = $i * k; $i += 1 }` — runs the
    /// entire insert loop natively, reserving hash capacity once and writing `(stringified i, i*k)`
    /// pairs in tight Rust. Emitted when the body is exactly
    /// `GetScalarSlot(i) + LoadInt(k) + Mul + GetScalarSlot(i) + SetHashElem(h) + Pop +
    /// SlotIncLtIntJumpBack(i, limit, body_target)` with `body_target` at the first `GetScalarSlot`.
    /// (hash_name_idx, i_slot, i32_multiplier, i32_limit)
    SetHashIntTimesLoop(u16, u8, i32, i32),
    /// Fused `$sum += $h{$k}` body op for the inner loop of `for my $k (keys %h) { $sum += $h{$k} }`.
    ///
    /// Replaces the 6-op sequence `GetScalarSlot(sum) + GetScalarPlain(k) + GetHashElem(h) + Add +
    /// SetScalarSlotKeep(sum) + Pop` with a single dispatch that reads the hash element directly
    /// into the slot without going through the VM stack. (sum_slot, k_name_idx, h_name_idx)
    AddHashElemPlainKeyToSlot(u8, u16, u16),
    /// Like [`Op::AddHashElemPlainKeyToSlot`] but the key variable lives in a slot (`for my $k`
    /// in slot-mode foreach). Pure slot read + hash lookup + slot write with zero VM stack traffic.
    /// (sum_slot, k_slot, h_name_idx)
    AddHashElemSlotKeyToSlot(u8, u8, u16),
    /// Fused `for my $k (keys %h) { $sum += $h{$k} }` — walks `hash.values()` in a tight native
    /// loop, accumulating integer or float sums directly into `sum_slot`. Emitted by the
    /// bytecode-level peephole when the foreach shape + `AddHashElemSlotKeyToSlot` body + slot
    /// counter/var declarations are detected. `h_name_idx` is the source hash's name pool index.
    /// (sum_slot, h_name_idx)
    SumHashValuesToSlot(u8, u16),

    // ── Frame-local scalar slots (O(1) access, no string lookup) ──
    /// Read scalar from current frame's slot array. u8 = slot index.
    GetScalarSlot(u8),
    /// Write scalar to current frame's slot array (pop, discard). u8 = slot index.
    SetScalarSlot(u8),
    /// Write scalar to current frame's slot array (pop, keep on stack). u8 = slot index.
    SetScalarSlotKeep(u8),
    /// Declare + initialize scalar in current frame's slot array. u8 = slot index; u16 = name pool
    /// index (bare name) for closure capture.
    DeclareScalarSlot(u8, u16),
    /// Read argument from caller's stack region: push stack\[call_frame.stack_base + idx\].
    /// Avoids @_ allocation + string-based shift for compiled sub argument passing.
    GetArg(u8),
    /// `reverse` in list context — stack: \[list\] → \[reversed list\]
    ReverseListOp,
    /// `scalar reverse` — stack: \[list\] → concatenated string with chars reversed (Perl).
    ReverseScalarOp,
    /// `rev` in list context — reverse list, preserve iterators lazily.
    RevListOp,
    /// `rev` in scalar context — char-reverse string.
    RevScalarOp,
    /// Pop TOS (array/list), push `to_list().len()` as integer (Perl `scalar` on map/grep result).
    StackArrayLen,
    /// Pop list-slice result array; push last element (Perl `scalar (LIST)[i,...]`).
    ListSliceToScalar,
    /// pmap { BLOCK } @list — block_idx; stack: \[progress_flag, list\] → \[mapped\] (`progress_flag` is 0/1)
    PMapWithBlock(u16),
    /// pflat_map { BLOCK } @list — flatten array results; output in **input order**; stack same as [`Op::PMapWithBlock`]
    PFlatMapWithBlock(u16),
    /// pmaps { BLOCK } LIST — streaming parallel map; stack: \[list\] → \[iterator\]
    PMapsWithBlock(u16),
    /// pflat_maps { BLOCK } LIST — streaming parallel flat map; stack: \[list\] → \[iterator\]
    PFlatMapsWithBlock(u16),
    /// `pmap_on` / `pflat_map_on` over SSH — stack: \[progress_flag, list, cluster\] → \[mapped\]; `flat` = 1 for flatten
    PMapRemote {
        block_idx: u16,
        flat: u8,
    },
    /// puniq LIST — hash-partition parallel distinct (first occurrence order); stack: \[progress_flag, list\] → \[array\]
    Puniq,
    /// pfirst { BLOCK } LIST — short-circuit parallel; stack: \[progress_flag, list\] → value or undef
    PFirstWithBlock(u16),
    /// pany { BLOCK } LIST — short-circuit parallel; stack: \[progress_flag, list\] → 0/1
    PAnyWithBlock(u16),
    /// pmap_chunked N { BLOCK } @list — block_idx; stack: \[progress_flag, chunk_n, list\] → \[mapped\]
    PMapChunkedWithBlock(u16),
    /// pgrep { BLOCK } @list — block_idx; stack: \[progress_flag, list\] → \[filtered\]
    PGrepWithBlock(u16),
    /// pgreps { BLOCK } LIST — streaming parallel grep; stack: \[list\] → \[iterator\]
    PGrepsWithBlock(u16),
    /// pfor { BLOCK } @list — block_idx; stack: \[progress_flag, list\] → \[\]
    PForWithBlock(u16),
    /// psort { BLOCK } @list — block_idx; stack: \[progress_flag, list\] → \[sorted\]
    PSortWithBlock(u16),
    /// psort @list (no block) — stack: \[progress_flag, list\] → \[sorted\]
    PSortNoBlockParallel,
    /// `reduce { BLOCK } @list` — block_idx; stack: \[list\] → \[accumulator\]
    ReduceWithBlock(u16),
    /// `preduce { BLOCK } @list` — block_idx; stack: \[progress_flag, list\] → \[accumulator\]
    PReduceWithBlock(u16),
    /// `preduce_init EXPR, { BLOCK } @list` — block_idx; stack: \[progress_flag, list, init\] → \[accumulator\]
    PReduceInitWithBlock(u16),
    /// `pmap_reduce { MAP } { REDUCE } @list` — map and reduce block indices; stack: \[progress_flag, list\] → \[scalar\]
    PMapReduceWithBlocks(u16, u16),
    /// `pcache { BLOCK } @list` — block_idx; stack: \[progress_flag, list\] → \[array\]
    PcacheWithBlock(u16),
    /// `pselect($rx1, ... [, timeout => SECS])` — stack: \[rx0, …, rx_{n-1}\] with optional timeout on top
    Pselect {
        n_rx: u8,
        has_timeout: bool,
    },
    /// `par_lines PATH, fn { } [, progress => EXPR]` — index into [`Chunk::par_lines_entries`]; stack: \[\] → `undef`
    ParLines(u16),
    /// `par_walk PATH, fn { } [, progress => EXPR]` — index into [`Chunk::par_walk_entries`]; stack: \[\] → `undef`
    ParWalk(u16),
    /// `pwatch GLOB, fn { }` — index into [`Chunk::pwatch_entries`]; stack: \[\] → result
    Pwatch(u16),
    /// fan N { BLOCK } — block_idx; stack: \[progress_flag, count\] (`progress_flag` is 0/1)
    FanWithBlock(u16),
    /// fan { BLOCK } — block_idx; stack: \[progress_flag\]; COUNT = rayon pool size (`stryke -j`)
    FanWithBlockAuto(u16),
    /// fan_cap N { BLOCK } — like fan; stack: \[progress_flag, count\] → array of block return values
    FanCapWithBlock(u16),
    /// fan_cap { BLOCK } — like fan; stack: \[progress_flag\] → array
    FanCapWithBlockAuto(u16),
    /// `do { BLOCK }` — block_idx + wantarray byte ([`crate::interpreter::WantarrayCtx::as_byte`]);
    /// stack: \[\] → result
    EvalBlock(u16, u8),
    /// `trace { BLOCK }` — block_idx; stack: \[\] → block value (stderr tracing for mysync mutations)
    TraceBlock(u16),
    /// `timer { BLOCK }` — block_idx; stack: \[\] → elapsed ms as float
    TimerBlock(u16),
    /// `bench { BLOCK } N` — block_idx; stack: \[iterations\] → benchmark summary string
    BenchBlock(u16),
    /// `given (EXPR) { when ... default ... }` — [`Chunk::given_entries`] /
    /// [`Chunk::given_topic_bytecode_ranges`]; stack: \[\] → topic result
    Given(u16),
    /// `eval_timeout SECS { ... }` — index into [`Chunk::eval_timeout_entries`] /
    /// [`Chunk::eval_timeout_expr_bytecode_ranges`]; stack: \[\] → block value
    EvalTimeout(u16),
    /// Algebraic `match (SUBJECT) { ... }` — [`Chunk::algebraic_match_entries`] /
    /// [`Chunk::algebraic_match_subject_bytecode_ranges`]; stack: \[\] → arm value
    AlgebraicMatch(u16),
    /// `async { BLOCK }` / `spawn { BLOCK }` — block_idx; stack: \[\] → AsyncTask
    AsyncBlock(u16),
    /// `await EXPR` — stack: \[value\] → result
    Await,
    /// `__SUB__` — push reference to currently executing sub (for anonymous recursion).
    LoadCurrentSub,
    /// `defer { BLOCK }` — register a block to run when the current scope exits.
    /// Stack: `[coderef]` → `[]`. The coderef is pushed to the frame's defer list.
    DeferBlock,
    /// Make a scalar reference from TOS (copies value into a new `RwLock`).
    MakeScalarRef,
    /// `\$name` when `name` is a plain scalar variable — ref aliases the live binding (same as tree `scalar_binding_ref`).
    MakeScalarBindingRef(u16),
    /// `\@name` — ref aliases the live array in scope (name pool index, stash-qualified like [`Op::GetArray`]).
    MakeArrayBindingRef(u16),
    /// `\%name` — ref aliases the live hash in scope.
    MakeHashBindingRef(u16),
    /// `\@{ EXPR }` after `EXPR` is on the stack — ARRAY ref aliasing the same storage as Perl (ref to existing ref or package array).
    MakeArrayRefAlias,
    /// `\%{ EXPR }` — HASH ref alias (same semantics as [`Op::MakeArrayRefAlias`] for hashes).
    MakeHashRefAlias,
    /// Make an array reference from TOS (which should be an Array)
    MakeArrayRef,
    /// Make a hash reference from TOS (which should be a Hash)
    MakeHashRef,
    /// Make an anonymous sub from a block — block_idx; stack: \[\] → CodeRef
    /// Anonymous `sub` / coderef: block pool index + [`Chunk::code_ref_sigs`] index (may be empty vec).
    MakeCodeRef(u16, u16),
    /// Push a code reference to a named sub (`\&foo`) — name pool index; resolves at run time.
    LoadNamedSubRef(u16),
    /// `\&{ EXPR }` — stack: \[sub name string\] → code ref (resolves at run time).
    LoadDynamicSubRef,
    /// `*{ EXPR }` — stack: \[stash / glob name string\] → resolved handle string (IO alias map + identity).
    LoadDynamicTypeglob,
    /// `*lhs = *rhs` — copy stash slots (sub, scalar, array, hash, IO alias); name pool indices for both sides.
    CopyTypeglobSlots(u16, u16),
    /// `*name = $coderef` — stack: pop value, install subroutine in typeglob, push value back (assignment result).
    TypeglobAssignFromValue(u16),
    /// `*{LHS} = $coderef` — stack: pop value, pop LHS glob name string, install sub, push value back.
    TypeglobAssignFromValueDynamic,
    /// `*{LHS} = *rhs` — stack: pop LHS glob name string; RHS name is pool index; copies stash like [`Op::CopyTypeglobSlots`].
    CopyTypeglobSlotsDynamicLhs(u16),
    /// Symbolic deref (`$$r`, `@{...}`, `%{...}`, `*{...}`): stack: \[ref or name value\] → result.
    /// Byte: `0` = [`crate::ast::Sigil::Scalar`], `1` = Array, `2` = Hash, `3` = Typeglob.
    SymbolicDeref(u8),
    /// Dereference arrow: ->\[\] — stack: \[ref, index\] → value
    ArrowArray,
    /// Dereference arrow: ->{} — stack: \[ref, key\] → value
    ArrowHash,
    /// Assign to `->{}`: stack: \[value, ref, key\] (key on top) — consumes three values.
    SetArrowHash,
    /// Assign to `->[]`: stack: \[value, ref, index\] (index on top) — consumes three values.
    SetArrowArray,
    /// Like [`Op::SetArrowArray`] but leaves the assigned value on the stack (for `++$aref->[$i]` value).
    SetArrowArrayKeep,
    /// Like [`Op::SetArrowHash`] but leaves the assigned value on the stack (for `++$href->{k}` value).
    SetArrowHashKeep,
    /// Postfix `++` / `--` on `->[]`: stack \[ref, index\] (index on top) → old value; mutates slot.
    /// Byte: `0` = increment, `1` = decrement.
    ArrowArrayPostfix(u8),
    /// Postfix `++` / `--` on `->{}`: stack \[ref, key\] (key on top) → old value; mutates slot.
    /// Byte: `0` = increment, `1` = decrement.
    ArrowHashPostfix(u8),
    /// `$$r = $val` — stack: \[value, ref\] (ref on top).
    SetSymbolicScalarRef,
    /// Like [`Op::SetSymbolicScalarRef`] but leaves the assigned value on the stack.
    SetSymbolicScalarRefKeep,
    /// `@{ EXPR } = LIST` — stack: \[list value, ref-or-name\] (top = ref / package name); delegates to
    /// [`Interpreter::assign_symbolic_array_ref_deref`](crate::interpreter::Interpreter::assign_symbolic_array_ref_deref).
    SetSymbolicArrayRef,
    /// `%{ EXPR } = LIST` — stack: \[list value, ref-or-name\]; pairs from list like `%h = (k => v, …)`.
    SetSymbolicHashRef,
    /// `*{ EXPR } = RHS` — stack: \[value, ref-or-name\] (top = symbolic glob name); coderef install or `*lhs = *rhs` copy.
    SetSymbolicTypeglobRef,
    /// Postfix `++` / `--` on symbolic scalar ref (`$$r`); stack \[ref\] → old value. Byte: `0` = increment, `1` = decrement.
    SymbolicScalarRefPostfix(u8),
    /// Dereference arrow: ->() — stack: \[ref, args_array\] → value
    /// `$cr->(...)` — wantarray byte (see VM `WantarrayCtx` threading on `Call` / `MethodCall`).
    ArrowCall(u8),
    /// Indirect call `$coderef(ARG...)` / `&$coderef(ARG...)` — stack (bottom→top): `target`, then
    /// `argc` argument values (first arg pushed first). Third byte: `1` = ignore stack args and use
    /// caller `@_` (`argc` must be `0`).
    IndirectCall(u8, u8, u8),
    /// Method call: stack: \[object, args...\] → result; name_idx, argc, wantarray
    MethodCall(u16, u8, u8),
    /// Like [`Op::MethodCall`] but uses SUPER / C3 parent chain (see interpreter method resolution for `SUPER`).
    MethodCallSuper(u16, u8, u8),
    /// File test: -e, -f, -d, etc. — test char; stack: \[path\] → 0/1
    FileTestOp(u8),

    // ── try / catch / finally (VM exception handling; see [`VM::try_recover_from_exception`]) ──
    /// Push a [`crate::vm::TryFrame`]; `catch_ip` / `after_ip` patched via [`Chunk::patch_try_push_catch`]
    /// / [`Chunk::patch_try_push_after`]; `finally_ip` via [`Chunk::patch_try_push_finally`].
    TryPush {
        catch_ip: usize,
        finally_ip: Option<usize>,
        after_ip: usize,
        catch_var_idx: u16,
    },
    /// Normal completion from try or catch body (jump to finally or merge).
    TryContinueNormal,
    /// End of `finally` block: pop try frame and jump to `after_ip`.
    TryFinallyEnd,
    /// Enter catch: consume [`crate::vm::VM::pending_catch_error`], pop try scope, push catch scope, bind `$var`.
    CatchReceive(u16),

    // ── `mysync` (thread-safe shared bindings; see [`StmtKind::MySync`]) ──
    /// Stack: `[init]` → `[]`. Declares `${name}` as `PerlValue::atomic` (or deque/heap unwrapped).
    DeclareMySyncScalar(u16),
    /// Stack: `[init_list]` → `[]`. Declares `@name` as atomic array.
    DeclareMySyncArray(u16),
    /// Stack: `[init_list]` → `[]`. Declares `%name` as atomic hash.
    DeclareMySyncHash(u16),
    /// Register [`RuntimeSubDecl`] at index (nested `sub`, including inside `BEGIN`).
    RuntimeSubDecl(u16),
    /// Register [`RuntimeAdviceDecl`] at index — install AOP advice into VM `intercepts` registry.
    RegisterAdvice(u16),
    /// `tie $x | @arr | %h, 'Class', ...` — stack bottom = class expr, then user args; `argc` = `1 + args.len()`.
    /// `target_kind`: 0 = scalar (`TIESCALAR`), 1 = array (`TIEARRAY`), 2 = hash (`TIEHASH`). `name_idx` = bare name.
    Tie {
        target_kind: u8,
        name_idx: u16,
        argc: u8,
    },
    /// `format NAME =` … — index into [`Chunk::format_decls`]; installs into current package at run time.
    FormatDecl(u16),
    /// `use overload 'op' => 'method', …` — index into [`Chunk::use_overload_entries`].
    UseOverload(u16),
    /// Scalar `$x OP= $rhs` — uses [`Scope::atomic_mutate`] so `mysync` scalars are RMW-safe.
    /// Stack: `[rhs]` → `[result]`. `op` byte is from [`crate::compiler::scalar_compound_op_to_byte`].
    ScalarCompoundAssign {
        name_idx: u16,
        op: u8,
    },

    // ── Special ──
    /// Set `${^GLOBAL_PHASE}` on the interpreter. See [`GP_START`] … [`GP_END`].
    SetGlobalPhase(u8),
    Halt,
    /// Delegate an AST expression to `Interpreter::eval_expr_ctx` at runtime.
    /// Operand is an index into [`Chunk::ast_eval_exprs`].
    EvalAstExpr(u16),

    // ── Streaming map (appended — do not reorder earlier op tags) ─────────────
    /// `maps { BLOCK } LIST` — stack: \[list\] → lazy iterator (pull-based; stryke extension).
    MapsWithBlock(u16),
    /// `flat_maps { BLOCK } LIST` — like [`Op::MapsWithBlock`] with `flat_map`-style flattening.
    MapsFlatMapWithBlock(u16),
    /// `maps EXPR, LIST` — index into [`Chunk::map_expr_entries`]; stack: \[list\] → iterator.
    MapsWithExpr(u16),
    /// `flat_maps EXPR, LIST` — same pools as [`Op::MapsWithExpr`].
    MapsFlatMapWithExpr(u16),
    /// `filter` / `fi` `{ BLOCK } LIST` — stack: \[list\] → lazy iterator (stryke; `grep` remains eager).
    FilterWithBlock(u16),
    /// `filter` / `fi` `EXPR, LIST` — index into [`Chunk::grep_expr_entries`]; stack: \[list\] → iterator.
    FilterWithExpr(u16),
}

/// `${^GLOBAL_PHASE}` values emitted with [`Op::SetGlobalPhase`] (matches Perl’s phase strings).
pub const GP_START: u8 = 0;
/// Reserved; stock Perl 5 keeps `${^GLOBAL_PHASE}` as **`START`** during `UNITCHECK` blocks.
pub const GP_UNITCHECK: u8 = 1;
pub const GP_CHECK: u8 = 2;
pub const GP_INIT: u8 = 3;
pub const GP_RUN: u8 = 4;
pub const GP_END: u8 = 5;

/// Built-in function IDs for CallBuiltin dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum BuiltinId {
    // String
    Length = 0,
    Chomp,
    Chop,
    Substr,
    Index,
    Rindex,
    Uc,
    Lc,
    Ucfirst,
    Lcfirst,
    Chr,
    Ord,
    Hex,
    Oct,
    Join,
    Split,
    Sprintf,

    // Numeric
    Abs,
    Int,
    Sqrt,

    // Type
    Defined,
    Ref,
    Scalar,

    // Array
    Splice,
    Reverse,
    Sort,
    Unshift,

    // Hash

    // I/O
    Open,
    Close,
    Eof,
    ReadLine,
    Printf,

    // System
    System,
    Exec,
    Exit,
    Die,
    Warn,
    Chdir,
    Mkdir,
    Unlink,

    // Control
    Eval,
    Do,
    Require,

    // OOP
    Bless,
    Caller,

    // Parallel
    PMap,
    PGrep,
    PFor,
    PSort,
    Fan,

    // Map/Grep (block-based — need special handling)
    MapBlock,
    GrepBlock,
    SortBlock,

    // Math (appended — do not reorder earlier IDs)
    Sin,
    Cos,
    Atan2,
    Exp,
    Log,
    Rand,
    Srand,

    // String (appended)
    Crypt,
    Fc,
    Pos,
    Study,

    Stat,
    Lstat,
    Link,
    Symlink,
    Readlink,
    Glob,

    Opendir,
    Readdir,
    Closedir,
    Rewinddir,
    Telldir,
    Seekdir,
    /// Read entire file as UTF-8 (`slurp $path`).
    Slurp,
    /// Blocking HTTP GET (`fetch_url $url`).
    FetchUrl,
    /// `pchannel()` — `(tx, rx)` as a two-element list.
    Pchannel,
    /// Parallel recursive glob (`glob_par`).
    GlobPar,
    /// `deque()` — empty deque.
    DequeNew,
    /// `heap(fn { })` — empty heap with comparator.
    HeapNew,
    /// `pipeline(...)` — lazy iterator (filter/map/take/collect).
    Pipeline,
    /// `capture("cmd")` — structured stdout/stderr/exit (via `sh -c`).
    Capture,
    /// `ppool(N)` — persistent thread pool (`submit` / `collect`).
    Ppool,
    /// Scalar/list context query (`wantarray`).
    Wantarray,
    /// `rename OLD, NEW`
    Rename,
    /// `chmod MODE, ...`
    Chmod,
    /// `chown UID, GID, ...`
    Chown,
    /// `pselect($rx1, $rx2, ...)` — multiplexed recv; returns `(value, index)`.
    Pselect,
    /// `barrier(N)` — thread barrier (`->wait`).
    BarrierNew,
    /// `par_pipeline(...)` — list form: same as `pipeline` but parallel `filter`/`map` on `collect()`.
    ParPipeline,
    /// `glob_par(..., progress => EXPR)` — last stack arg is truthy progress flag.
    GlobParProgress,
    /// `par_pipeline_stream(...)` — streaming pipeline with bounded channels between stages.
    ParPipelineStream,
    /// `par_sed(PATTERN, REPLACEMENT, FILES...)` — parallel in-place regex substitution per file.
    ParSed,
    /// `par_sed(..., progress => EXPR)` — last stack arg is truthy progress flag.
    ParSedProgress,
    /// `each EXPR` — returns empty list.
    Each,
    /// `` `cmd` `` / `qx{...}` — stdout string via `sh -c` (Perl readpipe); sets `$?`.
    Readpipe,
    /// `readline` / `<HANDLE>` in **list** context — all remaining lines until EOF (Perl `readline` list semantics).
    ReadLineList,
    /// `readdir` in **list** context — all names not yet returned (Perl drains the rest of the stream).
    ReaddirList,
    /// `ssh HOST, CMD, …` / `ssh(HOST, …)` — `execvp` style `ssh` only (no shell).
    Ssh,
    /// `rmdir LIST` — remove empty directories; returns count removed (appended ID).
    Rmdir,
    /// `utime ATIME, MTIME, LIST` — set access/mod times (Unix).
    Utime,
    /// `umask EXPR` / `umask()` — process file mode creation mask (Unix).
    Umask,
    /// `getcwd` / `pwd` — bare-name builtin returning the absolute current working directory.
    Getcwd,
    /// `pipe READHANDLE, WRITEHANDLE` — OS pipe ends (Unix).
    Pipe,
    /// `files` / `files DIR` — list file names in a directory (default: `.`).
    Files,
    /// `filesf` / `filesf DIR` / `f` — list only regular file names in a directory (default: `.`).
    Filesf,
    /// `fr DIR` — list only regular file names recursively (default: `.`).
    FilesfRecursive,
    /// `dirs` / `dirs DIR` / `d` — list subdirectory names in a directory (default: `.`).
    Dirs,
    /// `dr DIR` — list subdirectory paths recursively (default: `.`).
    DirsRecursive,
    /// `sym_links` / `sym_links DIR` — list symlink names in a directory (default: `.`).
    SymLinks,
    /// `sockets` / `sockets DIR` — list Unix socket names in a directory (default: `.`).
    Sockets,
    /// `pipes` / `pipes DIR` — list named-pipe (FIFO) names in a directory (default: `.`).
    Pipes,
    /// `block_devices` / `block_devices DIR` — list block device names in a directory (default: `.`).
    BlockDevices,
    /// `char_devices` / `char_devices DIR` — list character device names in a directory (default: `.`).
    CharDevices,
    /// `exe` / `exe DIR` — list executable file names in a directory (default: `.`).
    Executables,
}

impl BuiltinId {
    pub fn from_u16(v: u16) -> Option<Self> {
        if v <= Self::Executables as u16 {
            Some(unsafe { std::mem::transmute::<u16, BuiltinId>(v) })
        } else {
            None
        }
    }
}

/// A compiled chunk of bytecode with its constant pools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub ops: Vec<Op>,
    /// Constant pool: string literals, regex patterns, etc.
    #[serde(with = "crate::script_cache::constants_pool_codec")]
    pub constants: Vec<PerlValue>,
    /// Name pool: variable names, sub names (interned/deduped).
    pub names: Vec<String>,
    /// Source line for each op (parallel array for error reporting).
    pub lines: Vec<usize>,
    /// Optional link from each op to the originating [`Expr`] (pool index into [`Self::ast_expr_pool`]).
    /// Filled for ops emitted from [`crate::compiler::Compiler::compile_expr_ctx`]; other paths leave `None`.
    pub op_ast_expr: Vec<Option<u32>>,
    /// Interned [`Expr`] nodes referenced by [`Self::op_ast_expr`] (for debugging / tooling).
    pub ast_expr_pool: Vec<Expr>,
    /// Compiled subroutine entry points: (name_index, op_index, uses_stack_args).
    /// When `uses_stack_args` is true, the Call op leaves arguments on the value
    /// stack and the sub reads them via `GetArg(idx)` instead of `shift @_`.
    pub sub_entries: Vec<(u16, usize, bool)>,
    /// AST blocks for map/grep/sort/parallel operations.
    /// Referenced by block-based opcodes via u16 index.
    pub blocks: Vec<Block>,
    /// When `Some((start, end))`, `blocks[i]` is also lowered to `ops[start..end]` (exclusive `end`)
    /// with trailing [`Op::BlockReturnValue`]. VM uses opcodes; otherwise the AST in `blocks[i]`.
    pub block_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Resolved [`Op::CallStaticSubId`] targets: subroutine entry IP, stack-args calling convention,
    /// and stash name pool index (qualified key matching [`Interpreter::subs`]).
    pub static_sub_calls: Vec<(usize, bool, u16)>,
    /// Assign targets for `s///` / `tr///` bytecode (LHS expressions).
    pub lvalues: Vec<Expr>,
    /// AST expressions delegated to interpreter at runtime via [`Op::EvalAstExpr`].
    pub ast_eval_exprs: Vec<Expr>,
    /// Instruction pointer where the main program body starts (after BEGIN/CHECK/INIT phase blocks).
    /// Used by `-n`/`-p` line mode to re-execute only the body per input line.
    pub body_start_ip: usize,
    /// `struct Name { ... }` definitions in this chunk (registered on the interpreter at VM start).
    pub struct_defs: Vec<StructDef>,
    /// `enum Name { ... }` definitions in this chunk (registered on the interpreter at VM start).
    pub enum_defs: Vec<EnumDef>,
    /// `class Name extends ... impl ... { ... }` definitions.
    pub class_defs: Vec<ClassDef>,
    /// `trait Name { ... }` definitions.
    pub trait_defs: Vec<TraitDef>,
    /// `given (topic) { body }` — topic expression + body (when/default handled by interpreter).
    pub given_entries: Vec<(Expr, Block)>,
    /// When `Some((start, end))`, `given_entries[i].0` (topic) is lowered to `ops[start..end]` +
    /// [`Op::BlockReturnValue`].
    pub given_topic_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// `eval_timeout timeout_expr { body }` — evaluated at runtime.
    pub eval_timeout_entries: Vec<(Expr, Block)>,
    /// When `Some((start, end))`, `eval_timeout_entries[i].0` (timeout expr) is lowered to
    /// `ops[start..end]` with trailing [`Op::BlockReturnValue`].
    pub eval_timeout_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Algebraic `match (subject) { arms }`.
    pub algebraic_match_entries: Vec<(Expr, Vec<MatchArm>)>,
    /// When `Some((start, end))`, `algebraic_match_entries[i].0` (subject) is lowered to
    /// `ops[start..end]` + [`Op::BlockReturnValue`].
    pub algebraic_match_subject_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Nested / runtime `sub` declarations (see [`Op::RuntimeSubDecl`]).
    pub runtime_sub_decls: Vec<RuntimeSubDecl>,
    /// AOP advice declarations (see [`Op::RegisterAdvice`]).
    pub runtime_advice_decls: Vec<RuntimeAdviceDecl>,
    /// Stryke `fn ($a, …)` / hash-destruct params for [`Op::MakeCodeRef`] (second operand is pool index).
    pub code_ref_sigs: Vec<Vec<SubSigParam>>,
    /// `par_lines PATH, fn { } [, progress => EXPR]` — evaluated by interpreter inside VM.
    pub par_lines_entries: Vec<(Expr, Expr, Option<Expr>)>,
    /// `par_walk PATH, fn { } [, progress => EXPR]` — evaluated by interpreter inside VM.
    pub par_walk_entries: Vec<(Expr, Expr, Option<Expr>)>,
    /// `pwatch GLOB, fn { }` — evaluated by interpreter inside VM.
    pub pwatch_entries: Vec<(Expr, Expr)>,
    /// `substr $var, OFF, LEN, REPL` — four-arg form (mutates `LHS`); evaluated by interpreter inside VM.
    pub substr_four_arg_entries: Vec<(Expr, Expr, Option<Expr>, Expr)>,
    /// `keys EXPR` when `EXPR` is not bare `%h`.
    pub keys_expr_entries: Vec<Expr>,
    /// When `Some((start, end))`, `keys_expr_entries[i]` is lowered to `ops[start..end]` +
    /// [`Op::BlockReturnValue`] (operand only; [`Op::KeysExpr`] still applies `keys` to the value).
    pub keys_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// `values EXPR` when not bare `%h`.
    pub values_expr_entries: Vec<Expr>,
    pub values_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// `delete EXPR` when not the fast `%h{k}` lowering.
    pub delete_expr_entries: Vec<Expr>,
    /// `exists EXPR` when not the fast `%h{k}` lowering.
    pub exists_expr_entries: Vec<Expr>,
    /// `push` when the array operand is not a bare `@name` (e.g. `push $aref, ...`).
    pub push_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pub pop_expr_entries: Vec<Expr>,
    pub shift_expr_entries: Vec<Expr>,
    pub unshift_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pub splice_expr_entries: Vec<SpliceExprEntry>,
    /// `map EXPR, LIST` — map expression (list context) with `$_` set to each element.
    pub map_expr_entries: Vec<Expr>,
    /// When `Some((start, end))`, `map_expr_entries[i]` is lowered like [`Self::grep_expr_bytecode_ranges`].
    pub map_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// `grep EXPR, LIST` — filter expression evaluated with `$_` set to each element.
    pub grep_expr_entries: Vec<Expr>,
    /// When `Some((start, end))`, `grep_expr_entries[i]` is also lowered to `ops[start..end]`
    /// (exclusive `end`) with trailing [`Op::BlockReturnValue`], like [`Self::block_bytecode_ranges`].
    pub grep_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Right-hand expression for [`Op::RegexFlipFlopExprRhs`] — boolean context (bare `m//` is `$_ =~ m//`).
    pub regex_flip_flop_rhs_expr_entries: Vec<Expr>,
    /// When `Some((start, end))`, `regex_flip_flop_rhs_expr_entries[i]` is lowered to `ops[start..end]` +
    /// [`Op::BlockReturnValue`].
    pub regex_flip_flop_rhs_expr_bytecode_ranges: Vec<Option<(usize, usize)>>,
    /// Number of flip-flop slots ([`Op::ScalarFlipFlop`], [`Op::RegexFlipFlop`], [`Op::RegexEofFlipFlop`],
    /// [`Op::RegexFlipFlopExprRhs`], [`Op::RegexFlipFlopDotLineRhs`]); VM resets flip-flop vectors.
    pub flip_flop_slots: u16,
    /// `format NAME =` bodies: basename + lines between `=` and `.` (see lexer).
    pub format_decls: Vec<(String, Vec<String>)>,
    /// `use overload` pair lists (installed into current package at run time).
    pub use_overload_entries: Vec<Vec<(String, String)>>,
}

impl Chunk {
    /// Look up a compiled subroutine entry by stash name pool index.
    pub fn find_sub_entry(&self, name_idx: u16) -> Option<(usize, bool)> {
        self.sub_entries
            .iter()
            .find(|(n, _, _)| *n == name_idx)
            .map(|(_, ip, stack_args)| (*ip, *stack_args))
    }

    pub fn new() -> Self {
        Self {
            ops: Vec::with_capacity(256),
            constants: Vec::new(),
            names: Vec::new(),
            lines: Vec::new(),
            op_ast_expr: Vec::new(),
            ast_expr_pool: Vec::new(),
            sub_entries: Vec::new(),
            blocks: Vec::new(),
            block_bytecode_ranges: Vec::new(),
            static_sub_calls: Vec::new(),
            lvalues: Vec::new(),
            ast_eval_exprs: Vec::new(),
            body_start_ip: 0,
            struct_defs: Vec::new(),
            enum_defs: Vec::new(),
            class_defs: Vec::new(),
            trait_defs: Vec::new(),
            given_entries: Vec::new(),
            given_topic_bytecode_ranges: Vec::new(),
            eval_timeout_entries: Vec::new(),
            eval_timeout_expr_bytecode_ranges: Vec::new(),
            algebraic_match_entries: Vec::new(),
            algebraic_match_subject_bytecode_ranges: Vec::new(),
            runtime_sub_decls: Vec::new(),
            runtime_advice_decls: Vec::new(),
            code_ref_sigs: Vec::new(),
            par_lines_entries: Vec::new(),
            par_walk_entries: Vec::new(),
            pwatch_entries: Vec::new(),
            substr_four_arg_entries: Vec::new(),
            keys_expr_entries: Vec::new(),
            keys_expr_bytecode_ranges: Vec::new(),
            values_expr_entries: Vec::new(),
            values_expr_bytecode_ranges: Vec::new(),
            delete_expr_entries: Vec::new(),
            exists_expr_entries: Vec::new(),
            push_expr_entries: Vec::new(),
            pop_expr_entries: Vec::new(),
            shift_expr_entries: Vec::new(),
            unshift_expr_entries: Vec::new(),
            splice_expr_entries: Vec::new(),
            map_expr_entries: Vec::new(),
            map_expr_bytecode_ranges: Vec::new(),
            grep_expr_entries: Vec::new(),
            grep_expr_bytecode_ranges: Vec::new(),
            regex_flip_flop_rhs_expr_entries: Vec::new(),
            regex_flip_flop_rhs_expr_bytecode_ranges: Vec::new(),
            flip_flop_slots: 0,
            format_decls: Vec::new(),
            use_overload_entries: Vec::new(),
        }
    }

    /// Pool index for [`Op::FormatDecl`].
    pub fn add_format_decl(&mut self, name: String, lines: Vec<String>) -> u16 {
        let idx = self.format_decls.len() as u16;
        self.format_decls.push((name, lines));
        idx
    }

    /// Pool index for [`Op::UseOverload`].
    pub fn add_use_overload(&mut self, pairs: Vec<(String, String)>) -> u16 {
        let idx = self.use_overload_entries.len() as u16;
        self.use_overload_entries.push(pairs);
        idx
    }

    /// Allocate a slot index for [`Op::ScalarFlipFlop`] / [`Op::RegexFlipFlop`] / [`Op::RegexEofFlipFlop`] /
    /// [`Op::RegexFlipFlopExprRhs`] / [`Op::RegexFlipFlopDotLineRhs`] flip-flop state.
    pub fn alloc_flip_flop_slot(&mut self) -> u16 {
        let id = self.flip_flop_slots;
        self.flip_flop_slots = self.flip_flop_slots.saturating_add(1);
        id
    }

    /// `map EXPR, LIST` — pool index for [`Op::MapWithExpr`].
    pub fn add_map_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.map_expr_entries.len() as u16;
        self.map_expr_entries.push(expr);
        idx
    }

    /// `grep EXPR, LIST` — pool index for [`Op::GrepWithExpr`].
    pub fn add_grep_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.grep_expr_entries.len() as u16;
        self.grep_expr_entries.push(expr);
        idx
    }

    /// Regex flip-flop with compound RHS — pool index for [`Op::RegexFlipFlopExprRhs`].
    pub fn add_regex_flip_flop_rhs_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.regex_flip_flop_rhs_expr_entries.len() as u16;
        self.regex_flip_flop_rhs_expr_entries.push(expr);
        idx
    }

    /// `keys EXPR` (dynamic) — pool index for [`Op::KeysExpr`].
    pub fn add_keys_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.keys_expr_entries.len() as u16;
        self.keys_expr_entries.push(expr);
        idx
    }

    /// `values EXPR` (dynamic) — pool index for [`Op::ValuesExpr`].
    pub fn add_values_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.values_expr_entries.len() as u16;
        self.values_expr_entries.push(expr);
        idx
    }

    /// `delete EXPR` (dynamic operand) — pool index for [`Op::DeleteExpr`].
    pub fn add_delete_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.delete_expr_entries.len() as u16;
        self.delete_expr_entries.push(expr);
        idx
    }

    /// `exists EXPR` (dynamic operand) — pool index for [`Op::ExistsExpr`].
    pub fn add_exists_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.exists_expr_entries.len() as u16;
        self.exists_expr_entries.push(expr);
        idx
    }

    pub fn add_push_expr_entry(&mut self, array: Expr, values: Vec<Expr>) -> u16 {
        let idx = self.push_expr_entries.len() as u16;
        self.push_expr_entries.push((array, values));
        idx
    }

    pub fn add_pop_expr_entry(&mut self, array: Expr) -> u16 {
        let idx = self.pop_expr_entries.len() as u16;
        self.pop_expr_entries.push(array);
        idx
    }

    pub fn add_shift_expr_entry(&mut self, array: Expr) -> u16 {
        let idx = self.shift_expr_entries.len() as u16;
        self.shift_expr_entries.push(array);
        idx
    }

    pub fn add_unshift_expr_entry(&mut self, array: Expr, values: Vec<Expr>) -> u16 {
        let idx = self.unshift_expr_entries.len() as u16;
        self.unshift_expr_entries.push((array, values));
        idx
    }

    pub fn add_splice_expr_entry(
        &mut self,
        array: Expr,
        offset: Option<Expr>,
        length: Option<Expr>,
        replacement: Vec<Expr>,
    ) -> u16 {
        let idx = self.splice_expr_entries.len() as u16;
        self.splice_expr_entries
            .push((array, offset, length, replacement));
        idx
    }

    /// Four-arg `substr` — returns pool index for [`Op::SubstrFourArg`].
    pub fn add_substr_four_arg_entry(
        &mut self,
        string: Expr,
        offset: Expr,
        length: Option<Expr>,
        replacement: Expr,
    ) -> u16 {
        let idx = self.substr_four_arg_entries.len() as u16;
        self.substr_four_arg_entries
            .push((string, offset, length, replacement));
        idx
    }

    /// `par_lines PATH, fn { } [, progress => EXPR]` — returns pool index for [`Op::ParLines`].
    pub fn add_par_lines_entry(
        &mut self,
        path: Expr,
        callback: Expr,
        progress: Option<Expr>,
    ) -> u16 {
        let idx = self.par_lines_entries.len() as u16;
        self.par_lines_entries.push((path, callback, progress));
        idx
    }

    /// `par_walk PATH, fn { } [, progress => EXPR]` — returns pool index for [`Op::ParWalk`].
    pub fn add_par_walk_entry(
        &mut self,
        path: Expr,
        callback: Expr,
        progress: Option<Expr>,
    ) -> u16 {
        let idx = self.par_walk_entries.len() as u16;
        self.par_walk_entries.push((path, callback, progress));
        idx
    }

    /// `pwatch GLOB, fn { }` — returns pool index for [`Op::Pwatch`].
    pub fn add_pwatch_entry(&mut self, path: Expr, callback: Expr) -> u16 {
        let idx = self.pwatch_entries.len() as u16;
        self.pwatch_entries.push((path, callback));
        idx
    }

    /// `given (EXPR) { ... }` — returns pool index for [`Op::Given`].
    pub fn add_given_entry(&mut self, topic: Expr, body: Block) -> u16 {
        let idx = self.given_entries.len() as u16;
        self.given_entries.push((topic, body));
        idx
    }

    /// `eval_timeout SECS { ... }` — returns pool index for [`Op::EvalTimeout`].
    pub fn add_eval_timeout_entry(&mut self, timeout: Expr, body: Block) -> u16 {
        let idx = self.eval_timeout_entries.len() as u16;
        self.eval_timeout_entries.push((timeout, body));
        idx
    }

    /// Algebraic `match` — returns pool index for [`Op::AlgebraicMatch`].
    pub fn add_algebraic_match_entry(&mut self, subject: Expr, arms: Vec<MatchArm>) -> u16 {
        let idx = self.algebraic_match_entries.len() as u16;
        self.algebraic_match_entries.push((subject, arms));
        idx
    }

    /// Store an AST block and return its index.
    pub fn add_block(&mut self, block: Block) -> u16 {
        let idx = self.blocks.len() as u16;
        self.blocks.push(block);
        idx
    }

    /// Pool index for [`Op::MakeCodeRef`] signature (`stryke` extension); use empty vec for legacy `fn { }`.
    pub fn add_code_ref_sig(&mut self, params: Vec<SubSigParam>) -> u16 {
        let idx = self.code_ref_sigs.len();
        if idx > u16::MAX as usize {
            panic!("too many anonymous sub signatures in one chunk");
        }
        self.code_ref_sigs.push(params);
        idx as u16
    }

    /// Store an assignable expression (LHS of `s///` / `tr///`) and return its index.
    pub fn add_lvalue_expr(&mut self, e: Expr) -> u16 {
        let idx = self.lvalues.len() as u16;
        self.lvalues.push(e);
        idx
    }

    /// Intern a name, returning its pool index.
    pub fn intern_name(&mut self, name: &str) -> u16 {
        if let Some(idx) = self.names.iter().position(|n| n == name) {
            return idx as u16;
        }
        let idx = self.names.len() as u16;
        self.names.push(name.to_string());
        idx
    }

    /// Add a constant to the pool, returning its index.
    pub fn add_constant(&mut self, val: PerlValue) -> u16 {
        // Dedup string constants
        if let Some(ref s) = val.as_str() {
            for (i, c) in self.constants.iter().enumerate() {
                if let Some(cs) = c.as_str() {
                    if cs == *s {
                        return i as u16;
                    }
                }
            }
        }
        let idx = self.constants.len() as u16;
        self.constants.push(val);
        idx
    }

    /// Append an op with source line info.
    #[inline]
    pub fn emit(&mut self, op: Op, line: usize) -> usize {
        self.emit_with_ast_idx(op, line, None)
    }

    /// Like [`Self::emit`] but attach an optional interned AST [`Expr`] pool index (see [`Self::op_ast_expr`]).
    #[inline]
    pub fn emit_with_ast_idx(&mut self, op: Op, line: usize, ast: Option<u32>) -> usize {
        let idx = self.ops.len();
        self.ops.push(op);
        self.lines.push(line);
        self.op_ast_expr.push(ast);
        idx
    }

    /// Resolve the originating expression for an instruction pointer, if recorded.
    #[inline]
    pub fn ast_expr_at(&self, ip: usize) -> Option<&Expr> {
        let id = (*self.op_ast_expr.get(ip)?)?;
        self.ast_expr_pool.get(id as usize)
    }

    /// Patch a jump instruction at `idx` to target the current position.
    pub fn patch_jump_here(&mut self, idx: usize) {
        let target = self.ops.len();
        self.patch_jump_to(idx, target);
    }

    /// Patch a jump instruction at `idx` to target an explicit op address.
    pub fn patch_jump_to(&mut self, idx: usize, target: usize) {
        match &mut self.ops[idx] {
            Op::Jump(ref mut t)
            | Op::JumpIfTrue(ref mut t)
            | Op::JumpIfFalse(ref mut t)
            | Op::JumpIfFalseKeep(ref mut t)
            | Op::JumpIfTrueKeep(ref mut t)
            | Op::JumpIfDefinedKeep(ref mut t) => *t = target,
            _ => panic!("patch_jump_to on non-jump op at {}", idx),
        }
    }

    pub fn patch_try_push_catch(&mut self, idx: usize, catch_ip: usize) {
        match &mut self.ops[idx] {
            Op::TryPush { catch_ip: c, .. } => *c = catch_ip,
            _ => panic!("patch_try_push_catch on non-TryPush op at {}", idx),
        }
    }

    pub fn patch_try_push_finally(&mut self, idx: usize, finally_ip: Option<usize>) {
        match &mut self.ops[idx] {
            Op::TryPush { finally_ip: f, .. } => *f = finally_ip,
            _ => panic!("patch_try_push_finally on non-TryPush op at {}", idx),
        }
    }

    pub fn patch_try_push_after(&mut self, idx: usize, after_ip: usize) {
        match &mut self.ops[idx] {
            Op::TryPush { after_ip: a, .. } => *a = after_ip,
            _ => panic!("patch_try_push_after on non-TryPush op at {}", idx),
        }
    }

    /// Current op count (next emit position).
    #[inline]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Human-readable listing: subroutine entry points and each op with its source line (javap / `dis`-style).
    pub fn disassemble(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        for (i, n) in self.names.iter().enumerate() {
            let _ = writeln!(out, "; name[{}] = {}", i, n);
        }
        let _ = writeln!(out, "; sub_entries:");
        for (ni, ip, stack_args) in &self.sub_entries {
            let name = self
                .names
                .get(*ni as usize)
                .map(|s| s.as_str())
                .unwrap_or("?");
            let _ = writeln!(out, ";   {} @ {} stack_args={}", name, ip, stack_args);
        }
        for (i, op) in self.ops.iter().enumerate() {
            let line = self.lines.get(i).copied().unwrap_or(0);
            let ast = self
                .op_ast_expr
                .get(i)
                .copied()
                .flatten()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "-".into());
            let _ = writeln!(out, "{:04} {:>5} {:>6}  {:?}", i, line, ast, op);
        }
        out
    }

    /// Peephole pass: fuse common multi-op sequences into single superinstructions,
    /// then compact by removing Nop slots and remapping all jump targets.
    pub fn peephole_fuse(&mut self) {
        let len = self.ops.len();
        if len < 2 {
            return;
        }
        // Pass 1: fuse OP + Pop → OPVoid
        let mut i = 0;
        while i + 1 < len {
            if matches!(self.ops[i + 1], Op::Pop) {
                let replacement = match &self.ops[i] {
                    Op::AddAssignSlotSlot(d, s) => Some(Op::AddAssignSlotSlotVoid(*d, *s)),
                    Op::PreIncSlot(s) => Some(Op::PreIncSlotVoid(*s)),
                    Op::ConcatAppendSlot(s) => Some(Op::ConcatAppendSlotVoid(*s)),
                    _ => None,
                };
                if let Some(op) = replacement {
                    self.ops[i] = op;
                    self.ops[i + 1] = Op::Nop;
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }
        // Pass 2: fuse multi-op patterns
        // Helper: check if any jump targets position `pos`.
        let has_jump_to = |ops: &[Op], pos: usize| -> bool {
            for op in ops {
                let t = match op {
                    Op::Jump(t)
                    | Op::JumpIfFalse(t)
                    | Op::JumpIfTrue(t)
                    | Op::JumpIfFalseKeep(t)
                    | Op::JumpIfTrueKeep(t)
                    | Op::JumpIfDefinedKeep(t) => Some(*t),
                    _ => None,
                };
                if t == Some(pos) {
                    return true;
                }
            }
            false
        };
        let len = self.ops.len();
        if len >= 4 {
            i = 0;
            while i + 3 < len {
                if let (
                    Op::GetScalarSlot(slot),
                    Op::LoadInt(n),
                    Op::NumLt,
                    Op::JumpIfFalse(target),
                ) = (
                    &self.ops[i],
                    &self.ops[i + 1],
                    &self.ops[i + 2],
                    &self.ops[i + 3],
                ) {
                    if let Ok(n32) = i32::try_from(*n) {
                        // Don't fuse if any jump targets the ops that will become Nop.
                        // This prevents breaking short-circuit &&/|| that jump to the
                        // JumpIfFalse for the while condition exit check.
                        if has_jump_to(&self.ops, i + 1)
                            || has_jump_to(&self.ops, i + 2)
                            || has_jump_to(&self.ops, i + 3)
                        {
                            i += 1;
                            continue;
                        }
                        let slot = *slot;
                        let target = *target;
                        self.ops[i] = Op::SlotLtIntJumpIfFalse(slot, n32, target);
                        self.ops[i + 1] = Op::Nop;
                        self.ops[i + 2] = Op::Nop;
                        self.ops[i + 3] = Op::Nop;
                        i += 4;
                        continue;
                    }
                }
                i += 1;
            }
        }
        // Compact once so that pass 3 sees a Nop-free op stream and can match
        // adjacent `PreIncSlotVoid + Jump` backedges produced by passes 1/2.
        self.compact_nops();
        // Pass 3: fuse loop backedge
        //   PreIncSlotVoid(s)  + Jump(top)
        // where ops[top] is SlotLtIntJumpIfFalse(s, limit, exit)
        // becomes
        //   SlotIncLtIntJumpBack(s, limit, top + 1)   // body falls through
        //   Nop                                       // was Jump
        // The first-iteration check at `top` is still reached from before the loop
        // (the loop's initial entry goes through the top test), so leaving
        // SlotLtIntJumpIfFalse in place keeps the entry path correct. All
        // subsequent iterations now skip both the inc op and the jump.
        let len = self.ops.len();
        if len >= 2 {
            let mut i = 0;
            while i + 1 < len {
                if let (Op::PreIncSlotVoid(s), Op::Jump(top)) = (&self.ops[i], &self.ops[i + 1]) {
                    let slot = *s;
                    let top = *top;
                    // Only fuse backward branches — the C-style `for` shape where `top` is
                    // the loop's `SlotLtIntJumpIfFalse` test and the body falls through to
                    // this trailing increment. A forward `Jump` that happens to land on a
                    // similar test is not the same shape and must not be rewritten.
                    if top < i {
                        if let Op::SlotLtIntJumpIfFalse(tslot, limit, exit) = &self.ops[top] {
                            // Safety: the top test's exit target must equal the fused op's
                            // fall-through (i + 2). Otherwise exiting the loop via
                            // "condition false" would land somewhere the unfused shape never
                            // exited to.
                            if *tslot == slot && *exit == i + 2 {
                                let limit = *limit;
                                let body_target = top + 1;
                                self.ops[i] = Op::SlotIncLtIntJumpBack(slot, limit, body_target);
                                self.ops[i + 1] = Op::Nop;
                                i += 2;
                                continue;
                            }
                        }
                    }
                }
                i += 1;
            }
        }
        // Pass 4: compact again — remove the Nops introduced by pass 3.
        self.compact_nops();
        // Pass 5: fuse counted-loop bodies down to a single native superinstruction.
        //
        // After pass 3 + compact, a `for (my $i = ..; $i < N; $i = $i + 1) { $sum += $i }`
        // loop looks like:
        //
        //     [top]        SlotLtIntJumpIfFalse(i, N, exit)
        //     [body_start] AddAssignSlotSlotVoid(sum, i)       ← target of the backedge
        //                  SlotIncLtIntJumpBack(i, N, body_start)
        //     [exit]       ...
        //
        // When the body is exactly one op, we fuse the AddAssign + backedge into
        // `AccumSumLoop(sum, i, N)`, whose handler runs the whole remaining loop in a
        // tight Rust `while`. Same scheme for the counted `$s .= CONST` pattern, fused
        // into `ConcatConstSlotLoop`.
        //
        // Safety gate: only fire when no op jumps *into* the body (other than the backedge
        // itself and the top test's fall-through, which isn't a jump). That keeps loops with
        // interior labels / `last LABEL` / `next LABEL` from being silently skipped.
        let len = self.ops.len();
        if len >= 2 {
            let has_inbound_jump = |ops: &[Op], pos: usize, ignore: usize| -> bool {
                for (j, op) in ops.iter().enumerate() {
                    if j == ignore {
                        continue;
                    }
                    let t = match op {
                        Op::Jump(t)
                        | Op::JumpIfFalse(t)
                        | Op::JumpIfTrue(t)
                        | Op::JumpIfFalseKeep(t)
                        | Op::JumpIfTrueKeep(t)
                        | Op::JumpIfDefinedKeep(t) => Some(*t),
                        Op::SlotLtIntJumpIfFalse(_, _, t) => Some(*t),
                        Op::SlotIncLtIntJumpBack(_, _, t) => Some(*t),
                        _ => None,
                    };
                    if t == Some(pos) {
                        return true;
                    }
                }
                false
            };
            // 5a: AddAssignSlotSlotVoid + SlotIncLtIntJumpBack → AccumSumLoop
            let mut i = 0;
            while i + 1 < len {
                if let (
                    Op::AddAssignSlotSlotVoid(sum_slot, src_slot),
                    Op::SlotIncLtIntJumpBack(inc_slot, limit, body_target),
                ) = (&self.ops[i], &self.ops[i + 1])
                {
                    if *src_slot == *inc_slot
                        && *body_target == i
                        && !has_inbound_jump(&self.ops, i, i + 1)
                        && !has_inbound_jump(&self.ops, i + 1, i + 1)
                    {
                        let sum_slot = *sum_slot;
                        let src_slot = *src_slot;
                        let limit = *limit;
                        self.ops[i] = Op::AccumSumLoop(sum_slot, src_slot, limit);
                        self.ops[i + 1] = Op::Nop;
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            }
            // 5b: LoadConst + ConcatAppendSlotVoid + SlotIncLtIntJumpBack → ConcatConstSlotLoop
            if len >= 3 {
                let mut i = 0;
                while i + 2 < len {
                    if let (
                        Op::LoadConst(const_idx),
                        Op::ConcatAppendSlotVoid(s_slot),
                        Op::SlotIncLtIntJumpBack(inc_slot, limit, body_target),
                    ) = (&self.ops[i], &self.ops[i + 1], &self.ops[i + 2])
                    {
                        if *body_target == i
                            && !has_inbound_jump(&self.ops, i, i + 2)
                            && !has_inbound_jump(&self.ops, i + 1, i + 2)
                            && !has_inbound_jump(&self.ops, i + 2, i + 2)
                        {
                            let const_idx = *const_idx;
                            let s_slot = *s_slot;
                            let inc_slot = *inc_slot;
                            let limit = *limit;
                            self.ops[i] =
                                Op::ConcatConstSlotLoop(const_idx, s_slot, inc_slot, limit);
                            self.ops[i + 1] = Op::Nop;
                            self.ops[i + 2] = Op::Nop;
                            i += 3;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
            // 5e: `$sum += $h{$k}` body op inside `for my $k (keys %h) { ... }`
            //   GetScalarSlot(sum) + GetScalarPlain(k) + GetHashElem(h) + Add
            //     + SetScalarSlotKeep(sum) + Pop
            //   → AddHashElemPlainKeyToSlot(sum, k, h)
            // Safe because `SetScalarSlotKeep + Pop` leaves nothing on the stack net; the fused
            // op is a drop-in for that sequence. No inbound jumps permitted to interior ops.
            if len >= 6 {
                let mut i = 0;
                while i + 5 < len {
                    if let (
                        Op::GetScalarSlot(sum_slot),
                        Op::GetScalarPlain(k_idx),
                        Op::GetHashElem(h_idx),
                        Op::Add,
                        Op::SetScalarSlotKeep(sum_slot2),
                        Op::Pop,
                    ) = (
                        &self.ops[i],
                        &self.ops[i + 1],
                        &self.ops[i + 2],
                        &self.ops[i + 3],
                        &self.ops[i + 4],
                        &self.ops[i + 5],
                    ) {
                        if *sum_slot == *sum_slot2
                            && (0..6).all(|off| !has_inbound_jump(&self.ops, i + off, usize::MAX))
                        {
                            let sum_slot = *sum_slot;
                            let k_idx = *k_idx;
                            let h_idx = *h_idx;
                            self.ops[i] = Op::AddHashElemPlainKeyToSlot(sum_slot, k_idx, h_idx);
                            for off in 1..=5 {
                                self.ops[i + off] = Op::Nop;
                            }
                            i += 6;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
            // 5e-slot: slot-key variant of 5e, emitted when the compiler lowers `$k` (the foreach
            // loop variable) into a slot rather than a frame scalar.
            //   GetScalarSlot(sum) + GetScalarSlot(k) + GetHashElem(h) + Add
            //     + SetScalarSlotKeep(sum) + Pop
            //   → AddHashElemSlotKeyToSlot(sum, k, h)
            if len >= 6 {
                let mut i = 0;
                while i + 5 < len {
                    if let (
                        Op::GetScalarSlot(sum_slot),
                        Op::GetScalarSlot(k_slot),
                        Op::GetHashElem(h_idx),
                        Op::Add,
                        Op::SetScalarSlotKeep(sum_slot2),
                        Op::Pop,
                    ) = (
                        &self.ops[i],
                        &self.ops[i + 1],
                        &self.ops[i + 2],
                        &self.ops[i + 3],
                        &self.ops[i + 4],
                        &self.ops[i + 5],
                    ) {
                        if *sum_slot == *sum_slot2
                            && *sum_slot != *k_slot
                            && (0..6).all(|off| !has_inbound_jump(&self.ops, i + off, usize::MAX))
                        {
                            let sum_slot = *sum_slot;
                            let k_slot = *k_slot;
                            let h_idx = *h_idx;
                            self.ops[i] = Op::AddHashElemSlotKeyToSlot(sum_slot, k_slot, h_idx);
                            for off in 1..=5 {
                                self.ops[i + off] = Op::Nop;
                            }
                            i += 6;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
            // 5d: counted hash-insert loop `$h{$i} = $i * K`
            //   GetScalarSlot(i) + LoadInt(k) + Mul + GetScalarSlot(i) + SetHashElem(h) + Pop
            //     + SlotIncLtIntJumpBack(i, limit, body_target)
            //   → SetHashIntTimesLoop(h, i, k, limit)
            if len >= 7 {
                let mut i = 0;
                while i + 6 < len {
                    if let (
                        Op::GetScalarSlot(gs1),
                        Op::LoadInt(k),
                        Op::Mul,
                        Op::GetScalarSlot(gs2),
                        Op::SetHashElem(h_idx),
                        Op::Pop,
                        Op::SlotIncLtIntJumpBack(inc_slot, limit, body_target),
                    ) = (
                        &self.ops[i],
                        &self.ops[i + 1],
                        &self.ops[i + 2],
                        &self.ops[i + 3],
                        &self.ops[i + 4],
                        &self.ops[i + 5],
                        &self.ops[i + 6],
                    ) {
                        if *gs1 == *inc_slot
                            && *gs2 == *inc_slot
                            && *body_target == i
                            && i32::try_from(*k).is_ok()
                            && (0..6).all(|off| !has_inbound_jump(&self.ops, i + off, i + 6))
                            && !has_inbound_jump(&self.ops, i + 6, i + 6)
                        {
                            let h_idx = *h_idx;
                            let inc_slot = *inc_slot;
                            let k32 = *k as i32;
                            let limit = *limit;
                            self.ops[i] = Op::SetHashIntTimesLoop(h_idx, inc_slot, k32, limit);
                            for off in 1..=6 {
                                self.ops[i + off] = Op::Nop;
                            }
                            i += 7;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
            // 5c: GetScalarSlot + PushArray + ArrayLen + Pop + SlotIncLtIntJumpBack
            //      → PushIntRangeToArrayLoop
            // This is the compiler's `push @a, $i; $i++` shape in void context, where
            // the `push` expression's length return is pushed by `ArrayLen` and then `Pop`ped.
            if len >= 5 {
                let mut i = 0;
                while i + 4 < len {
                    if let (
                        Op::GetScalarSlot(get_slot),
                        Op::PushArray(push_idx),
                        Op::ArrayLen(len_idx),
                        Op::Pop,
                        Op::SlotIncLtIntJumpBack(inc_slot, limit, body_target),
                    ) = (
                        &self.ops[i],
                        &self.ops[i + 1],
                        &self.ops[i + 2],
                        &self.ops[i + 3],
                        &self.ops[i + 4],
                    ) {
                        if *get_slot == *inc_slot
                            && *push_idx == *len_idx
                            && *body_target == i
                            && !has_inbound_jump(&self.ops, i, i + 4)
                            && !has_inbound_jump(&self.ops, i + 1, i + 4)
                            && !has_inbound_jump(&self.ops, i + 2, i + 4)
                            && !has_inbound_jump(&self.ops, i + 3, i + 4)
                            && !has_inbound_jump(&self.ops, i + 4, i + 4)
                        {
                            let push_idx = *push_idx;
                            let inc_slot = *inc_slot;
                            let limit = *limit;
                            self.ops[i] = Op::PushIntRangeToArrayLoop(push_idx, inc_slot, limit);
                            self.ops[i + 1] = Op::Nop;
                            self.ops[i + 2] = Op::Nop;
                            self.ops[i + 3] = Op::Nop;
                            self.ops[i + 4] = Op::Nop;
                            i += 5;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
        }
        // Pass 6: compact — remove the Nops pass 5 introduced.
        self.compact_nops();
        // Pass 7: fuse the entire `for my $k (keys %h) { $sum += $h{$k} }` loop into a single
        // `SumHashValuesToSlot` op that walks the hash's values in a tight native loop.
        //
        // After prior passes and compaction the shape is a 15-op block:
        //
        //     HashKeys(h)
        //     DeclareArray(list)
        //     LoadInt(0)
        //     DeclareScalarSlot(c, cname)
        //     LoadUndef
        //     DeclareScalarSlot(v, vname)
        //     [top]  GetScalarSlot(c)
        //            ArrayLen(list)
        //            NumLt
        //            JumpIfFalse(end)
        //            GetScalarSlot(c)
        //            GetArrayElem(list)
        //            SetScalarSlot(v)
        //            AddHashElemSlotKeyToSlot(sum, v, h)     ← fused body (pass 5e-slot)
        //            PreIncSlotVoid(c)
        //            Jump(top)
        //     [end]
        //
        // The counter (`__foreach_i__`), list (`__foreach_list__`), and loop var (`$k`) live
        // inside a `PushFrame`-isolated scope and are invisible after the loop — it is safe to
        // elide all of them. The fused op accumulates directly into `sum` without creating the
        // keys array at all.
        //
        // Safety gates:
        //   - `h` in HashKeys must match `h` in AddHashElemSlotKeyToSlot.
        //   - `list` in DeclareArray must match the loop `ArrayLen` / `GetArrayElem`.
        //   - `c` / `v` slots must be consistent throughout.
        //   - No inbound jump lands inside the 15-op window from the outside.
        //   - JumpIfFalse target must be i+15 (just past the Jump back-edge).
        //   - Jump back-edge target must be i+6 (the GetScalarSlot(c) at loop top).
        let len = self.ops.len();
        if len >= 15 {
            let has_inbound_jump =
                |ops: &[Op], pos: usize, ignore_from: usize, ignore_to: usize| -> bool {
                    for (j, op) in ops.iter().enumerate() {
                        if j >= ignore_from && j <= ignore_to {
                            continue;
                        }
                        let t = match op {
                            Op::Jump(t)
                            | Op::JumpIfFalse(t)
                            | Op::JumpIfTrue(t)
                            | Op::JumpIfFalseKeep(t)
                            | Op::JumpIfTrueKeep(t)
                            | Op::JumpIfDefinedKeep(t) => *t,
                            Op::SlotLtIntJumpIfFalse(_, _, t) => *t,
                            Op::SlotIncLtIntJumpBack(_, _, t) => *t,
                            _ => continue,
                        };
                        if t == pos {
                            return true;
                        }
                    }
                    false
                };
            let mut i = 0;
            while i + 15 < len {
                if let (
                    Op::HashKeys(h_idx),
                    Op::DeclareArray(list_idx),
                    Op::LoadInt(0),
                    Op::DeclareScalarSlot(c_slot, _c_name),
                    Op::LoadUndef,
                    Op::DeclareScalarSlot(v_slot, _v_name),
                    Op::GetScalarSlot(c_get1),
                    Op::ArrayLen(len_idx),
                    Op::NumLt,
                    Op::JumpIfFalse(end_tgt),
                    Op::GetScalarSlot(c_get2),
                    Op::GetArrayElem(elem_idx),
                    Op::SetScalarSlot(v_set),
                    Op::AddHashElemSlotKeyToSlot(sum_slot, v_in_body, h_in_body),
                    Op::PreIncSlotVoid(c_inc),
                    Op::Jump(top_tgt),
                ) = (
                    &self.ops[i],
                    &self.ops[i + 1],
                    &self.ops[i + 2],
                    &self.ops[i + 3],
                    &self.ops[i + 4],
                    &self.ops[i + 5],
                    &self.ops[i + 6],
                    &self.ops[i + 7],
                    &self.ops[i + 8],
                    &self.ops[i + 9],
                    &self.ops[i + 10],
                    &self.ops[i + 11],
                    &self.ops[i + 12],
                    &self.ops[i + 13],
                    &self.ops[i + 14],
                    &self.ops[i + 15],
                ) {
                    let full_end = i + 15;
                    if *list_idx == *len_idx
                        && *list_idx == *elem_idx
                        && *c_slot == *c_get1
                        && *c_slot == *c_get2
                        && *c_slot == *c_inc
                        && *v_slot == *v_set
                        && *v_slot == *v_in_body
                        && *h_idx == *h_in_body
                        && *top_tgt == i + 6
                        && *end_tgt == i + 16
                        && *sum_slot != *c_slot
                        && *sum_slot != *v_slot
                        && !(i..=full_end).any(|k| has_inbound_jump(&self.ops, k, i, full_end))
                    {
                        let sum_slot = *sum_slot;
                        let h_idx = *h_idx;
                        self.ops[i] = Op::SumHashValuesToSlot(sum_slot, h_idx);
                        for off in 1..=15 {
                            self.ops[i + off] = Op::Nop;
                        }
                        i += 16;
                        continue;
                    }
                }
                i += 1;
            }
        }
        // Pass 8: compact pass 7's Nops.
        self.compact_nops();
    }

    /// Remove all `Nop` instructions and remap jump targets + metadata indices.
    fn compact_nops(&mut self) {
        let old_len = self.ops.len();
        // Build old→new index mapping.
        let mut remap = vec![0usize; old_len + 1];
        let mut new_idx = 0usize;
        for (old, slot) in remap[..old_len].iter_mut().enumerate() {
            *slot = new_idx;
            if !matches!(self.ops[old], Op::Nop) {
                new_idx += 1;
            }
        }
        remap[old_len] = new_idx;
        if new_idx == old_len {
            return; // nothing to compact
        }
        // Remap jump targets in all ops.
        for op in &mut self.ops {
            match op {
                Op::Jump(t) | Op::JumpIfFalse(t) | Op::JumpIfTrue(t) => *t = remap[*t],
                Op::JumpIfFalseKeep(t) | Op::JumpIfTrueKeep(t) | Op::JumpIfDefinedKeep(t) => {
                    *t = remap[*t]
                }
                Op::SlotLtIntJumpIfFalse(_, _, t) => *t = remap[*t],
                Op::SlotIncLtIntJumpBack(_, _, t) => *t = remap[*t],
                _ => {}
            }
        }
        // Remap sub entry points.
        for e in &mut self.sub_entries {
            e.1 = remap[e.1];
        }
        // Remap `CallStaticSubId` resolved entry IPs — they were recorded by
        // `patch_static_sub_calls` before peephole fusion ran, so any Nop
        // removal in front of a sub body shifts its entry and must be
        // reflected here; otherwise `vm_dispatch_user_call` jumps one (or
        // more) ops past the real sub start and silently skips the first
        // instruction(s) of the body.
        for c in &mut self.static_sub_calls {
            c.0 = remap[c.0];
        }
        // Remap block/grep/sort/etc bytecode ranges.
        fn remap_ranges(ranges: &mut [Option<(usize, usize)>], remap: &[usize]) {
            for r in ranges.iter_mut().flatten() {
                r.0 = remap[r.0];
                r.1 = remap[r.1];
            }
        }
        remap_ranges(&mut self.block_bytecode_ranges, &remap);
        remap_ranges(&mut self.map_expr_bytecode_ranges, &remap);
        remap_ranges(&mut self.grep_expr_bytecode_ranges, &remap);
        remap_ranges(&mut self.keys_expr_bytecode_ranges, &remap);
        remap_ranges(&mut self.values_expr_bytecode_ranges, &remap);
        remap_ranges(&mut self.eval_timeout_expr_bytecode_ranges, &remap);
        remap_ranges(&mut self.given_topic_bytecode_ranges, &remap);
        remap_ranges(&mut self.algebraic_match_subject_bytecode_ranges, &remap);
        remap_ranges(&mut self.regex_flip_flop_rhs_expr_bytecode_ranges, &remap);
        // Compact ops, lines, op_ast_expr.
        let mut j = 0;
        for old in 0..old_len {
            if !matches!(self.ops[old], Op::Nop) {
                self.ops[j] = self.ops[old].clone();
                if old < self.lines.len() && j < self.lines.len() {
                    self.lines[j] = self.lines[old];
                }
                if old < self.op_ast_expr.len() && j < self.op_ast_expr.len() {
                    self.op_ast_expr[j] = self.op_ast_expr[old];
                }
                j += 1;
            }
        }
        self.ops.truncate(j);
        self.lines.truncate(j);
        self.op_ast_expr.truncate(j);
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast;

    #[test]
    fn chunk_new_and_default_match() {
        let a = Chunk::new();
        let b = Chunk::default();
        assert!(a.ops.is_empty() && a.names.is_empty() && a.constants.is_empty());
        assert!(b.ops.is_empty() && b.lines.is_empty());
    }

    #[test]
    fn intern_name_deduplicates() {
        let mut c = Chunk::new();
        let i0 = c.intern_name("foo");
        let i1 = c.intern_name("foo");
        let i2 = c.intern_name("bar");
        assert_eq!(i0, i1);
        assert_ne!(i0, i2);
        assert_eq!(c.names.len(), 2);
    }

    #[test]
    fn add_constant_dedups_identical_strings() {
        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::string("x".into()));
        let b = c.add_constant(PerlValue::string("x".into()));
        assert_eq!(a, b);
        assert_eq!(c.constants.len(), 1);
    }

    #[test]
    fn add_constant_distinct_strings_different_indices() {
        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::string("a".into()));
        let b = c.add_constant(PerlValue::string("b".into()));
        assert_ne!(a, b);
        assert_eq!(c.constants.len(), 2);
    }

    #[test]
    fn add_constant_non_string_no_dedup_scan() {
        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::integer(1));
        let b = c.add_constant(PerlValue::integer(1));
        assert_ne!(a, b);
        assert_eq!(c.constants.len(), 2);
    }

    #[test]
    fn emit_records_parallel_ops_and_lines() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(1), 10);
        c.emit(Op::Pop, 11);
        assert_eq!(c.len(), 2);
        assert_eq!(c.lines, vec![10, 11]);
        assert_eq!(c.op_ast_expr, vec![None, None]);
        assert!(!c.is_empty());
    }

    #[test]
    fn len_is_empty_track_ops() {
        let mut c = Chunk::new();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        c.emit(Op::Halt, 0);
        assert!(!c.is_empty());
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn patch_jump_here_updates_jump_target() {
        let mut c = Chunk::new();
        let j = c.emit(Op::Jump(0), 1);
        c.emit(Op::LoadInt(99), 2);
        c.patch_jump_here(j);
        assert_eq!(c.ops.len(), 2);
        assert!(matches!(c.ops[j], Op::Jump(2)));
    }

    #[test]
    fn patch_jump_here_jump_if_true() {
        let mut c = Chunk::new();
        let j = c.emit(Op::JumpIfTrue(0), 1);
        c.emit(Op::Halt, 2);
        c.patch_jump_here(j);
        assert!(matches!(c.ops[j], Op::JumpIfTrue(2)));
    }

    #[test]
    fn patch_jump_here_jump_if_false_keep() {
        let mut c = Chunk::new();
        let j = c.emit(Op::JumpIfFalseKeep(0), 1);
        c.emit(Op::Pop, 2);
        c.patch_jump_here(j);
        assert!(matches!(c.ops[j], Op::JumpIfFalseKeep(2)));
    }

    #[test]
    fn patch_jump_here_jump_if_true_keep() {
        let mut c = Chunk::new();
        let j = c.emit(Op::JumpIfTrueKeep(0), 1);
        c.emit(Op::Pop, 2);
        c.patch_jump_here(j);
        assert!(matches!(c.ops[j], Op::JumpIfTrueKeep(2)));
    }

    #[test]
    fn patch_jump_here_jump_if_defined_keep() {
        let mut c = Chunk::new();
        let j = c.emit(Op::JumpIfDefinedKeep(0), 1);
        c.emit(Op::Halt, 2);
        c.patch_jump_here(j);
        assert!(matches!(c.ops[j], Op::JumpIfDefinedKeep(2)));
    }

    #[test]
    #[should_panic(expected = "patch_jump_to on non-jump op")]
    fn patch_jump_here_panics_on_non_jump() {
        let mut c = Chunk::new();
        let idx = c.emit(Op::LoadInt(1), 1);
        c.patch_jump_here(idx);
    }

    #[test]
    fn add_block_returns_sequential_indices() {
        let mut c = Chunk::new();
        let b0: ast::Block = vec![];
        let b1: ast::Block = vec![];
        assert_eq!(c.add_block(b0), 0);
        assert_eq!(c.add_block(b1), 1);
        assert_eq!(c.blocks.len(), 2);
    }

    #[test]
    fn builtin_id_from_u16_first_and_last() {
        assert_eq!(BuiltinId::from_u16(0), Some(BuiltinId::Length));
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Pselect as u16),
            Some(BuiltinId::Pselect)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::BarrierNew as u16),
            Some(BuiltinId::BarrierNew)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::ParPipeline as u16),
            Some(BuiltinId::ParPipeline)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::GlobParProgress as u16),
            Some(BuiltinId::GlobParProgress)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Readpipe as u16),
            Some(BuiltinId::Readpipe)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::ReadLineList as u16),
            Some(BuiltinId::ReadLineList)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::ReaddirList as u16),
            Some(BuiltinId::ReaddirList)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Ssh as u16),
            Some(BuiltinId::Ssh)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Pipe as u16),
            Some(BuiltinId::Pipe)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Files as u16),
            Some(BuiltinId::Files)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Filesf as u16),
            Some(BuiltinId::Filesf)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Dirs as u16),
            Some(BuiltinId::Dirs)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::SymLinks as u16),
            Some(BuiltinId::SymLinks)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Sockets as u16),
            Some(BuiltinId::Sockets)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Pipes as u16),
            Some(BuiltinId::Pipes)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::BlockDevices as u16),
            Some(BuiltinId::BlockDevices)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::CharDevices as u16),
            Some(BuiltinId::CharDevices)
        );
        assert_eq!(
            BuiltinId::from_u16(BuiltinId::Executables as u16),
            Some(BuiltinId::Executables)
        );
    }

    #[test]
    fn builtin_id_from_u16_out_of_range() {
        assert_eq!(BuiltinId::from_u16(BuiltinId::Executables as u16 + 1), None);
        assert_eq!(BuiltinId::from_u16(u16::MAX), None);
    }

    #[test]
    fn op_enum_clone_roundtrip() {
        let o = Op::Call(42, 3, 0);
        assert!(matches!(o.clone(), Op::Call(42, 3, 0)));
    }

    #[test]
    fn chunk_clone_independent_ops() {
        let mut c = Chunk::new();
        c.emit(Op::Negate, 1);
        let mut d = c.clone();
        d.emit(Op::Pop, 2);
        assert_eq!(c.len(), 1);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn chunk_disassemble_includes_ops() {
        let mut c = Chunk::new();
        c.emit(Op::LoadInt(7), 1);
        let s = c.disassemble();
        assert!(s.contains("0000"));
        assert!(s.contains("LoadInt(7)"));
        assert!(s.contains("     -")); // no ast ref column
    }

    #[test]
    fn ast_expr_at_roundtrips_pooled_expr() {
        let mut c = Chunk::new();
        let e = ast::Expr {
            kind: ast::ExprKind::Integer(99),
            line: 3,
        };
        c.ast_expr_pool.push(e);
        c.emit_with_ast_idx(Op::LoadInt(99), 3, Some(0));
        let got = c.ast_expr_at(0).expect("ast ref");
        assert!(matches!(&got.kind, ast::ExprKind::Integer(99)));
        assert_eq!(got.line, 3);
    }
}
