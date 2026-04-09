use crate::ast::Block;
use crate::value::PerlValue;

/// Stack-based bytecode instruction set for the perlrs VM.
/// Operands use u16 for pool indices (64k names/constants) and i32 for jumps.
#[derive(Debug, Clone)]
pub enum Op {
    // ── Constants ──
    LoadInt(i64),
    LoadFloat(f64),
    LoadConst(u16), // index into constant pool
    LoadUndef,

    // ── Stack ──
    Pop,
    Dup,

    // ── Scalars (u16 = name pool index) ──
    GetScalar(u16),
    SetScalar(u16),
    DeclareScalar(u16),

    // ── Arrays ──
    GetArray(u16),
    SetArray(u16),
    DeclareArray(u16),
    GetArrayElem(u16), // stack: [index] → value
    SetArrayElem(u16), // stack: [value, index]
    PushArray(u16),    // stack: [value] → push to named array
    PopArray(u16),     // → popped value
    ShiftArray(u16),   // → shifted value
    ArrayLen(u16),     // → integer length

    // ── Hashes ──
    GetHash(u16),
    SetHash(u16),
    DeclareHash(u16),
    GetHashElem(u16),    // stack: [key] → value
    SetHashElem(u16),    // stack: [value, key]
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

    // ── Functions ──
    /// Call subroutine: name index, arg count
    Call(u16, u8),
    Return,
    ReturnValue,

    // ── Scope ──
    PushFrame,
    PopFrame,

    // ── I/O ──
    Print(u8), // arg count
    Say(u8),

    // ── Built-in function calls ──
    /// Calls a registered built-in: (builtin_id, arg_count)
    CallBuiltin(u16, u8),

    // ── List / Range ──
    MakeArray(u16), // pop N values, push as Array
    MakeHash(u16),  // pop N key-value pairs, push as Hash
    Range,          // stack: [from, to] → Array

    // ── Regex ──
    /// Match: pattern_const_idx, flags_const_idx; stack: string operand → result
    RegexMatch(u16, u16),

    // ── Assign helpers ──
    /// SetScalar that also leaves the value on the stack (for chained assignment)
    SetScalarKeep(u16),

    // ── Block-based operations (u16 = index into chunk.blocks) ──
    /// map { BLOCK } @list — block_idx; stack: \[list\] → \[mapped\]
    MapWithBlock(u16),
    /// grep { BLOCK } @list — block_idx; stack: \[list\] → \[filtered\]
    GrepWithBlock(u16),
    /// sort { BLOCK } @list — block_idx; stack: \[list\] → \[sorted\]
    SortWithBlock(u16),
    /// sort @list (no block) — stack: \[list\] → \[sorted\]
    SortNoBlock,
    /// reverse — stack: \[list\] → \[reversed\]
    ReverseOp,
    /// pmap { BLOCK } @list — block_idx; stack: \[list\] → \[mapped\]
    PMapWithBlock(u16),
    /// pgrep { BLOCK } @list — block_idx; stack: \[list\] → \[filtered\]
    PGrepWithBlock(u16),
    /// pfor { BLOCK } @list — block_idx; stack: \[list\]
    PForWithBlock(u16),
    /// psort { BLOCK } @list — block_idx; stack: \[list\] → \[sorted\]
    PSortWithBlock(u16),
    /// fan N { BLOCK } — block_idx; stack: \[count\]
    FanWithBlock(u16),
    /// eval { BLOCK } — block_idx; stack: \[\] → result
    EvalBlock(u16),
    /// Make a scalar reference from TOS
    MakeScalarRef,
    /// Make an array reference from TOS (which should be an Array)
    MakeArrayRef,
    /// Make a hash reference from TOS (which should be a Hash)
    MakeHashRef,
    /// Make an anonymous sub from a block — block_idx; stack: \[\] → CodeRef
    MakeCodeRef(u16),
    /// Dereference arrow: ->\[\] — stack: \[ref, index\] → value
    ArrowArray,
    /// Dereference arrow: ->{} — stack: \[ref, key\] → value
    ArrowHash,
    /// Dereference arrow: ->() — stack: \[ref, args_array\] → value
    ArrowCall,
    /// Method call: stack: \[object, args...\] → result; name_idx, argc
    MethodCall(u16, u8),
    /// File test: -e, -f, -d, etc. — test char; stack: \[path\] → 0/1
    FileTestOp(u8),

    // ── Special ──
    Halt,
}

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
}

impl BuiltinId {
    pub fn from_u16(v: u16) -> Option<Self> {
        if v <= Self::SortBlock as u16 {
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
    /// Compiled subroutine entry points: name_index → op_index
    pub sub_entries: Vec<(u16, usize)>,
    /// AST blocks for map/grep/sort/parallel operations.
    /// Referenced by block-based opcodes via u16 index.
    pub blocks: Vec<Block>,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            ops: Vec::with_capacity(256),
            constants: Vec::new(),
            names: Vec::new(),
            lines: Vec::new(),
            sub_entries: Vec::new(),
            blocks: Vec::new(),
        }
    }

    /// Store an AST block and return its index.
    pub fn add_block(&mut self, block: Block) -> u16 {
        let idx = self.blocks.len() as u16;
        self.blocks.push(block);
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
        if let PerlValue::String(ref s) = val {
            for (i, c) in self.constants.iter().enumerate() {
                if let PerlValue::String(ref cs) = c {
                    if cs == s {
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
        let idx = self.ops.len();
        self.ops.push(op);
        self.lines.push(line);
        idx
    }

    /// Patch a jump instruction at `idx` to target the current position.
    pub fn patch_jump_here(&mut self, idx: usize) {
        let target = self.ops.len();
        match &mut self.ops[idx] {
            Op::Jump(ref mut t)
            | Op::JumpIfTrue(ref mut t)
            | Op::JumpIfFalse(ref mut t)
            | Op::JumpIfFalseKeep(ref mut t)
            | Op::JumpIfTrueKeep(ref mut t)
            | Op::JumpIfDefinedKeep(ref mut t) => *t = target,
            _ => panic!("patch_jump_here on non-jump op at {}", idx),
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
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}
