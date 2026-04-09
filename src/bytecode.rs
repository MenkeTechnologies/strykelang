use crate::ast::Block;
use crate::value::PerlValue;

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

    // Math (appended — do not reorder earlier IDs)
    Sin,
    Cos,
    Atan2,
    Exp,
    Log,
    Rand,
    Srand,
}

impl BuiltinId {
    pub fn from_u16(v: u16) -> Option<Self> {
        if v <= Self::Srand as u16 {
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
        let a = c.add_constant(PerlValue::String("x".into()));
        let b = c.add_constant(PerlValue::String("x".into()));
        assert_eq!(a, b);
        assert_eq!(c.constants.len(), 1);
    }

    #[test]
    fn add_constant_distinct_strings_different_indices() {
        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::String("a".into()));
        let b = c.add_constant(PerlValue::String("b".into()));
        assert_ne!(a, b);
        assert_eq!(c.constants.len(), 2);
    }

    #[test]
    fn add_constant_non_string_no_dedup_scan() {
        let mut c = Chunk::new();
        let a = c.add_constant(PerlValue::Integer(1));
        let b = c.add_constant(PerlValue::Integer(1));
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
    #[should_panic(expected = "patch_jump_here on non-jump op")]
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
            BuiltinId::from_u16(BuiltinId::Srand as u16),
            Some(BuiltinId::Srand)
        );
    }

    #[test]
    fn builtin_id_from_u16_out_of_range() {
        assert_eq!(BuiltinId::from_u16(BuiltinId::Srand as u16 + 1), None);
        assert_eq!(BuiltinId::from_u16(u16::MAX), None);
    }

    #[test]
    fn op_enum_clone_roundtrip() {
        let o = Op::Call(42, 3);
        assert!(matches!(o.clone(), Op::Call(42, 3)));
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
}
