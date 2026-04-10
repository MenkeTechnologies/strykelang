use crate::ast::{Block, Expr, MatchArm, StructDef};
use crate::value::PerlValue;

/// `sub` body registered at run time (e.g. `BEGIN { sub f { ... } }`), mirrored from
/// [`crate::interpreter::Interpreter::exec_statement`] `StmtKind::SubDecl`.
#[derive(Debug, Clone)]
pub struct RuntimeSubDecl {
    pub name: String,
    pub params: Vec<String>,
    pub body: Block,
    pub prototype: Option<String>,
}

/// Stack-based bytecode instruction set for the perlrs VM.
/// Operands use u16 for pool indices (64k names/constants) and i32 for jumps.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
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

    // ── Arrays ──
    GetArray(u16),
    SetArray(u16),
    DeclareArray(u16),
    DeclareArrayFrozen(u16),
    GetArrayElem(u16), // stack: [index] → value
    SetArrayElem(u16), // stack: [value, index]
    /// Like [`Op::SetArrayElem`] but leaves the assigned value on the stack (e.g. `$a[$i] //=`).
    SetArrayElemKeep(u16),
    PushArray(u16),    // stack: [value] → push to named array
    PopArray(u16),     // → popped value
    ShiftArray(u16),   // → shifted value
    ArrayLen(u16),     // → integer length

    // ── Hashes ──
    GetHash(u16),
    SetHash(u16),
    DeclareHash(u16),
    DeclareHashFrozen(u16),
    /// Dynamic `local $x` — save previous binding, assign TOS (same stack shape as DeclareScalar).
    LocalDeclareScalar(u16),
    LocalDeclareArray(u16),
    LocalDeclareHash(u16),
    GetHashElem(u16),    // stack: [key] → value
    SetHashElem(u16),    // stack: [value, key]
    /// Like [`Op::SetHashElem`] but leaves the assigned value on the stack (e.g. `$h{k} //=`).
    SetHashElemKeep(u16),
    DeleteHashElem(u16), // stack: [key] → deleted value
    ExistsHashElem(u16), // stack: [key] → 0/1
    HashKeys(u16),       // → array of keys
    HashValues(u16),     // → array of values

    // ── Arithmetic ──
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Negate,

    // ── String ──
    Concat,
    StringRepeat,

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
    /// Closed-form `for (my $i=0; $i < limit; $i=$i+1) { $sum = $sum + $i }` with `limit >= 0`.
    /// Must follow [`Op::PushFrame`] and `my $i = 0`; `sum` is outer lexical, `i` inner.
    TriangularForAccum {
        limit: i64,
        sum_name_idx: u16,
        i_name_idx: u16,
    },

    // ── I/O ──
    Print(u8), // arg count
    Say(u8),

    // ── Built-in function calls ──
    /// Calls a registered built-in: (builtin_id, arg_count)
    CallBuiltin(u16, u8),

    // ── List / Range ──
    MakeArray(u16), // pop N values, push as Array
    /// `@$href{k1,k2}` — stack: `[container, key1, …, keyN]` (TOS = last key); pops `N+1` values; pushes array of slot values.
    HashSliceDeref(u16),
    /// `@$aref[i1,i2,...]` — stack: `[array_ref, i1, …, iN]` (TOS = last index); pops `N+1` values; pushes array of elements.
    ArrowArraySlice(u16),
    /// `@$href{k1,k2} = VALUE` — stack: `[value, container, key1, …, keyN]` (TOS = last key); pops `N+2` values.
    SetHashSliceDeref(u16),
    MakeHash(u16),  // pop N key-value pairs, push as Hash
    Range,          // stack: [from, to] → Array

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

    // ── Assign helpers ──
    /// SetScalar that also leaves the value on the stack (for chained assignment)
    SetScalarKeep(u16),
    /// `SetScalarKeep` for non-special scalars (see `SetScalarPlain`).
    SetScalarKeepPlain(u16),

    // ── Block-based operations (u16 = index into chunk.blocks) ──
    /// map { BLOCK } @list — block_idx; stack: \[list\] → \[mapped\]
    MapWithBlock(u16),
    /// grep { BLOCK } @list — block_idx; stack: \[list\] → \[filtered\]
    GrepWithBlock(u16),
    /// grep EXPR, LIST — index into [`Chunk::grep_expr_entries`]; stack: \[list\] → \[filtered\]
    GrepWithExpr(u16),
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
    /// `chomp` on assignable expr: stack has value → chomped count; uses `chunk.lvalues[idx]`.
    ChompInPlace(u16),
    /// `chop` on assignable expr: stack has value → chopped char; uses `chunk.lvalues[idx]`.
    ChopInPlace(u16),
    /// Four-arg `substr LHS, OFF, LEN, REPL` — index into [`Chunk::substr_four_arg_entries`]; stack: \[\] → extracted slice string
    SubstrFourArg(u16),
    /// `keys EXPR` when `EXPR` is not a bare `%h` — index into [`Chunk::keys_expr_entries`]
    KeysExpr(u16),
    /// `values EXPR` when not a bare `%h` — index into [`Chunk::values_expr_entries`]
    ValuesExpr(u16),
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
    /// reverse — stack: \[list\] → \[reversed\]
    ReverseOp,
    /// pmap { BLOCK } @list — block_idx; stack: \[progress_flag, list\] → \[mapped\] (`progress_flag` is 0/1)
    PMapWithBlock(u16),
    /// pmap_chunked N { BLOCK } @list — block_idx; stack: \[progress_flag, chunk_n, list\] → \[mapped\]
    PMapChunkedWithBlock(u16),
    /// pgrep { BLOCK } @list — block_idx; stack: \[progress_flag, list\] → \[filtered\]
    PGrepWithBlock(u16),
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
    /// `par_lines PATH, sub { } [, progress => EXPR]` — index into [`Chunk::par_lines_entries`]; stack: \[\] → `undef`
    ParLines(u16),
    /// `par_walk PATH, sub { } [, progress => EXPR]` — index into [`Chunk::par_walk_entries`]; stack: \[\] → `undef`
    ParWalk(u16),
    /// `pwatch GLOB, sub { }` — index into [`Chunk::pwatch_entries`]; stack: \[\] → result
    Pwatch(u16),
    /// fan N { BLOCK } — block_idx; stack: \[progress_flag, count\] (`progress_flag` is 0/1)
    FanWithBlock(u16),
    /// fan { BLOCK } — block_idx; stack: \[progress_flag\]; COUNT = rayon pool size (`pe -j`)
    FanWithBlockAuto(u16),
    /// fan_cap N { BLOCK } — like fan; stack: \[progress_flag, count\] → array of block return values
    FanCapWithBlock(u16),
    /// fan_cap { BLOCK } — like fan; stack: \[progress_flag\] → array
    FanCapWithBlockAuto(u16),
    /// eval { BLOCK } — block_idx; stack: \[\] → result
    EvalBlock(u16),
    /// `trace { BLOCK }` — block_idx; stack: \[\] → block value (stderr tracing for mysync mutations)
    TraceBlock(u16),
    /// `timer { BLOCK }` — block_idx; stack: \[\] → elapsed ms as float
    TimerBlock(u16),
    /// `bench { BLOCK } N` — block_idx; stack: \[iterations\] → benchmark summary string
    BenchBlock(u16),
    /// `given (EXPR) { when ... default ... }` — index into [`Chunk::given_entries`]; stack: \[\] → topic result
    Given(u16),
    /// `eval_timeout SECS { ... }` — index into [`Chunk::eval_timeout_entries`]; stack: \[\] → block value
    EvalTimeout(u16),
    /// Algebraic `match (SUBJECT) { ... }` — index into [`Chunk::algebraic_match_entries`]; stack: \[\] → arm value
    AlgebraicMatch(u16),
    /// `async { BLOCK }` / `spawn { BLOCK }` — block_idx; stack: \[\] → AsyncTask
    AsyncBlock(u16),
    /// `await EXPR` — stack: \[value\] → result
    Await,
    /// Make a scalar reference from TOS (copies value into a new `RwLock`).
    MakeScalarRef,
    /// `\$name` when `name` is a plain scalar variable — ref aliases the live binding (same as tree `scalar_binding_ref`).
    MakeScalarBindingRef(u16),
    /// Make an array reference from TOS (which should be an Array)
    MakeArrayRef,
    /// Make a hash reference from TOS (which should be a Hash)
    MakeHashRef,
    /// Make an anonymous sub from a block — block_idx; stack: \[\] → CodeRef
    MakeCodeRef(u16),
    /// Push a code reference to a named sub (`\&foo`) — name pool index; resolves at run time.
    LoadNamedSubRef(u16),
    /// `\&{ EXPR }` — stack: \[sub name string\] → code ref (resolves at run time).
    LoadDynamicSubRef,
    /// `*{ EXPR }` — stack: \[stash / glob name string\] → resolved handle string (IO alias map + identity).
    LoadDynamicTypeglob,
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
    /// `tie $x | @arr | %h, 'Class', ...` — stack bottom = class expr, then user args; `argc` = `1 + args.len()`.
    /// `target_kind`: 0 = scalar (`TIESCALAR`), 1 = array (`TIEARRAY`), 2 = hash (`TIEHASH`). `name_idx` = bare name.
    Tie {
        target_kind: u8,
        name_idx: u16,
        argc: u8,
    },
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// `heap(sub { })` — empty heap with comparator.
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
    /// `each EXPR` — matches tree interpreter (returns empty list).
    Each,
    /// `` `cmd` `` / `qx{...}` — stdout string via `sh -c` (Perl readpipe); sets `$?`.
    Readpipe,
}

impl BuiltinId {
    pub fn from_u16(v: u16) -> Option<Self> {
        if v <= Self::Readpipe as u16 {
            Some(unsafe { std::mem::transmute::<u16, BuiltinId>(v) })
        } else {
            None
        }
    }
}

/// A compiled chunk of bytecode with its constant pools.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub ops: Vec<Op>,
    /// Constant pool: string literals, regex patterns, etc.
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
    /// Assign targets for `s///` / `tr///` bytecode (LHS expressions).
    pub lvalues: Vec<Expr>,
    /// `struct Name { ... }` definitions in this chunk (registered on the interpreter at VM start).
    pub struct_defs: Vec<StructDef>,
    /// `given (topic) { body }` — topic expression + body (when/default handled by interpreter).
    pub given_entries: Vec<(Expr, Block)>,
    /// `eval_timeout timeout_expr { body }` — evaluated at runtime.
    pub eval_timeout_entries: Vec<(Expr, Block)>,
    /// Algebraic `match (subject) { arms }`.
    pub algebraic_match_entries: Vec<(Expr, Vec<MatchArm>)>,
    /// Nested / runtime `sub` declarations (see [`Op::RuntimeSubDecl`]).
    pub runtime_sub_decls: Vec<RuntimeSubDecl>,
    /// `par_lines PATH, sub { } [, progress => EXPR]` — evaluated by interpreter inside VM.
    pub par_lines_entries: Vec<(Expr, Expr, Option<Expr>)>,
    /// `par_walk PATH, sub { } [, progress => EXPR]` — evaluated by interpreter inside VM.
    pub par_walk_entries: Vec<(Expr, Expr, Option<Expr>)>,
    /// `pwatch GLOB, sub { }` — evaluated by interpreter inside VM.
    pub pwatch_entries: Vec<(Expr, Expr)>,
    /// `substr $var, OFF, LEN, REPL` — four-arg form (mutates `LHS`); evaluated by interpreter inside VM.
    pub substr_four_arg_entries: Vec<(Expr, Expr, Option<Expr>, Expr)>,
    /// `keys EXPR` when `EXPR` is not bare `%h`.
    pub keys_expr_entries: Vec<Expr>,
    /// `values EXPR` when not bare `%h`.
    pub values_expr_entries: Vec<Expr>,
    /// `delete EXPR` when not the fast `%h{k}` lowering.
    pub delete_expr_entries: Vec<Expr>,
    /// `exists EXPR` when not the fast `%h{k}` lowering.
    pub exists_expr_entries: Vec<Expr>,
    /// `push` when the array operand is not a bare `@name` (e.g. `push $aref, ...`).
    pub push_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pub pop_expr_entries: Vec<Expr>,
    pub shift_expr_entries: Vec<Expr>,
    pub unshift_expr_entries: Vec<(Expr, Vec<Expr>)>,
    pub splice_expr_entries: Vec<(Expr, Option<Expr>, Option<Expr>, Vec<Expr>)>,
    /// `grep EXPR, LIST` — filter expression evaluated with `$_` set to each element.
    pub grep_expr_entries: Vec<Expr>,
}

impl Chunk {
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
            lvalues: Vec::new(),
            struct_defs: Vec::new(),
            given_entries: Vec::new(),
            eval_timeout_entries: Vec::new(),
            algebraic_match_entries: Vec::new(),
            runtime_sub_decls: Vec::new(),
            par_lines_entries: Vec::new(),
            par_walk_entries: Vec::new(),
            pwatch_entries: Vec::new(),
            substr_four_arg_entries: Vec::new(),
            keys_expr_entries: Vec::new(),
            values_expr_entries: Vec::new(),
            delete_expr_entries: Vec::new(),
            exists_expr_entries: Vec::new(),
            push_expr_entries: Vec::new(),
            pop_expr_entries: Vec::new(),
            shift_expr_entries: Vec::new(),
            unshift_expr_entries: Vec::new(),
            splice_expr_entries: Vec::new(),
            grep_expr_entries: Vec::new(),
        }
    }

    /// `grep EXPR, LIST` — pool index for [`Op::GrepWithExpr`].
    pub fn add_grep_expr_entry(&mut self, expr: Expr) -> u16 {
        let idx = self.grep_expr_entries.len() as u16;
        self.grep_expr_entries.push(expr);
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

    /// `par_lines PATH, sub { } [, progress => EXPR]` — returns pool index for [`Op::ParLines`].
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

    /// `par_walk PATH, sub { } [, progress => EXPR]` — returns pool index for [`Op::ParWalk`].
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

    /// `pwatch GLOB, sub { }` — returns pool index for [`Op::Pwatch`].
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
    }

    #[test]
    fn builtin_id_from_u16_out_of_range() {
        assert_eq!(BuiltinId::from_u16(BuiltinId::Readpipe as u16 + 1), None);
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
