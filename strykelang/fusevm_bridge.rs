//! Bridge between strykelang bytecode (`crate::bytecode::Op`) and the shared
//! [`fusevm`] runtime.
//!
//! strykelang is a [`fusevm`] frontend: a conservative, universal-integer subset
//! of its bytecode is translated to `fusevm::Op` and executed on `fusevm::VM`,
//! reusing fusevm's interpreter and three-tier Cranelift JIT for the numeric hot
//! path. Everything fusevm cannot represent (strings, arrays, hashes, regex,
//! closures, calls, AOP, …) stays in strykelang's own VM in [`crate::vm`].
//!
//! Mirrors the awkrs/zshrs integration pattern: each frontend owns its own
//! `Extended(u16, u8)` ID space (see [`ext_ops`]); the universal ops translate
//! 1:1, so jump targets remap by a fixed preamble offset with no re-indexing.
//!
//! This tier sits ahead of strykelang's own linear/block sub-JIT in the dispatch
//! loop: it only fires on segments that sub-JIT would also accept, restricted
//! further to the strict 1:1 universal-integer/slot subset below, so its semantics
//! match the interpreter on the values it accepts.

use crate::bytecode::Op;
use crate::jit::SubTerminator;
use crate::value::StrykeValue;

/// strykelang's reserved `fusevm::Op::Extended(u16, u8)` extension-op ID space.
///
/// fusevm dispatches language-specific opcodes through a per-frontend handler
/// table keyed on the `u16` ID; strykelang owns the `0x0000..` block listed here
/// (zshrs and awkrs own disjoint blocks in their own bridges, so no IDs collide).
/// IDs are reserved here as strykelang's frontend surface; the universal subset
/// that this module currently executes needs none of them, but they pin the ID
/// space so future language-specific ops can be lowered to fusevm without
/// renumbering.
pub mod ext_ops {
    /// Perl-style regex match (`=~`).
    pub const STK_REGEX_MATCH: u16 = 0x0000;
    /// Perl-style regex substitution (`s///`).
    pub const STK_REGEX_SUBST: u16 = 0x0001;
    /// Hash slice (`@h{...}`).
    pub const STK_HASH_SLICE: u16 = 0x0002;
    /// Array slice (`@a[...]`).
    pub const STK_ARRAY_SLICE: u16 = 0x0003;
    /// String interpolation join.
    pub const STK_INTERP_JOIN: u16 = 0x0004;
    /// Wantarray context query.
    pub const STK_WANTARRAY: u16 = 0x0005;
    /// Perl/strykelang *floored* modulo (`%`). Lowered to `fusevm::Op::Extended`
    /// and JIT-compiled by [`super::StrykeJitExt`]; the floored semantics (result
    /// takes the sign of the divisor) differ from fusevm's truncated `Op::Mod`,
    /// which is why `%` is its own extension op rather than the universal `Mod`.
    pub const STK_MOD_FLOOR: u16 = 0x0006;

    /// String comparison ops (`eq ne lt gt le ge cmp`). Each lowers to an
    /// `Extended` op whose JIT/interpreter handler reconstructs the two operand
    /// `StrykeValue`s from their NaN-boxed bits (passed as i64 slot handles) and
    /// defers to strykelang's native [`crate::value::StrykeValue::str_eq`] /
    /// [`crate::value::StrykeValue::str_cmp`], so grapheme/Unicode semantics are
    /// preserved exactly. Results are `i64` (0/1, or -1/0/1 for `cmp`), which keeps
    /// the chunk on fusevm's integer block JIT + on-disk native cache.
    pub const STK_STR_EQ: u16 = 0x0007;
    /// String inequality (`ne`).
    pub const STK_STR_NE: u16 = 0x0008;
    /// String less-than (`lt`).
    pub const STK_STR_LT: u16 = 0x0009;
    /// String greater-than (`gt`).
    pub const STK_STR_GT: u16 = 0x000A;
    /// String less-or-equal (`le`).
    pub const STK_STR_LE: u16 = 0x000B;
    /// String greater-or-equal (`ge`).
    pub const STK_STR_GE: u16 = 0x000C;
    /// String three-way compare (`cmp`); yields -1/0/1.
    pub const STK_STR_CMP: u16 = 0x000D;

    /// String concatenation (`.`). Unlike the compare ops (which return an `i64`
    /// boolean/ordering), this *allocates* a new heap string and returns its raw
    /// NaN-boxed bits as an `i64` handle. The freshly-built `StrykeValue` is
    /// `mem::forget`-ed by the helper so its `Arc` ownership transfers into that
    /// handle; the bridge reconstructs an owning `StrykeValue` from the returned
    /// bits (see [`super::run_linear_segment`]). Only routed when both operands are
    /// plain strings (`is_string_like`), so the byte concatenation is identical to
    /// the interpreter and never triggers `use overload`/stringify side effects.
    pub const STK_STR_CONCAT: u16 = 0x000E;

    /// String `length`. Unary (`(i64 handle) -> i64 count`), unlike every other
    /// `STK_STR_*` op which is binary. Routed only when the operand is a plain
    /// string (`is_string_like`), so the handler/helper just stringifies and counts
    /// (chars under the `utf8` pragma, else bytes — see [`super::stryke_utf8_pragma`]).
    /// The result is an `i64`, keeping the chunk on the integer block JIT + disk cache.
    pub const STK_STR_LEN: u16 = 0x000F;
    /// String `ord` — first char's Unicode codepoint. Unary `(i64) -> i64`.
    pub const STK_STR_ORD: u16 = 0x0010;
    /// String `hex` — parse as hexadecimal. Unary `(i64) -> i64`.
    pub const STK_STR_HEX: u16 = 0x0011;
    /// String `oct` — parse as octal/hex/binary (Perl `oct`). Unary `(i64) -> i64`.
    pub const STK_STR_OCT: u16 = 0x0012;

    /// String `uc` — upper-case. Unary, but unlike length/ord/hex/oct it *allocates*
    /// a new owned string and returns its raw NaN-boxed bits as an `i64` handle (the
    /// helper `mem::forget`s the result, transferring `Arc` ownership; the bridge
    /// reconstructs one owning `StrykeValue` via `from_raw_bits`, exactly like
    /// `STK_STR_CONCAT`). Routed only when the operand is a plain string.
    pub const STK_STR_UC: u16 = 0x0013;
    /// String `lc` — lower-case. Unary string→string handle (see `STK_STR_UC`).
    pub const STK_STR_LC: u16 = 0x0014;
    /// String `ucfirst` — upper-case first char. Unary string→string handle.
    pub const STK_STR_UCFIRST: u16 = 0x0015;
    /// String `lcfirst` — lower-case first char. Unary string→string handle.
    pub const STK_STR_LCFIRST: u16 = 0x0016;
    /// `fc` — Unicode default case-fold. Unary string→string handle (see `STK_STR_UC`).
    pub const STK_STR_FC: u16 = 0x001A;
    /// `quotemeta` / `qm` — backslash-escape every non-word char (Perl regex
    /// shield, equivalent of `\Q…\E`). Unary string→string handle (see
    /// `STK_STR_UC`); reuses the same `(i64) -> i64` ABI and owned-handle return.
    pub const STK_STR_QUOTEMETA: u16 = 0x001E;
    /// `crypt($plaintext, $salt)` — DES/MD5 password hash. Binary
    /// string+string→string handle: both operands are NaN-boxed string handles,
    /// result is an owned string handle (see `STK_STR_CONCAT`). Routes through
    /// `crate::crypt_util::perl_crypt` (pure Rust, no FFI).
    pub const STK_STR_CRYPT: u16 = 0x001F;
    /// `reverse($s)` — reverse a scalar string character-by-character. Unary
    /// string→string handle (see `STK_STR_UC`). Only the scalar (string) form
    /// is lowered here; the list/array form stays on the interpreter (different
    /// result type, allocates a Vec).
    pub const STK_STR_REVERSE: u16 = 0x0020;
    /// `defined($x)` — 0 if `$x` is UNDEF, else 1. Unary **any-value**→int: the
    /// operand can be ANY type including UNDEF (unlike string→int ops which gate
    /// on `is_string_like` and bail for non-string operands). Marshaled as raw
    /// NaN-boxed `StrykeValue` bits and reconstructed by the helper.
    pub const STK_VAL_DEFINED: u16 = 0x0021;
    /// `ref($x)` — Perl-style type-name string: "ARRAY"/"HASH"/"SCALAR"/"CODE"/
    /// "Regexp" for refs, blessed-package name for objects, empty string for
    /// non-refs. Unary any-value→**string handle** — combines the defined-style
    /// any-value gate-bypass with the unary-str-str result reconstruction
    /// (`from_raw_bits` on the owned handle, like `STK_STR_UC` and friends).
    pub const STK_VAL_REF: u16 = 0x0022;
    /// `round($x)` (1-arg) — round to nearest integer, ties AWAY from zero
    /// (matches `f64::round() as i64`). Any-value→int (handles UNDEF/string/
    /// numeric uniformly via `to_number().round()`); same gate-bypass as
    /// `defined`. The 2-arg precision form stays on the interpreter.
    pub const STK_VAL_ROUND: u16 = 0x0023;
    /// `substr($s, $off)` (2-arg) — byte-offset suffix. Binary string+INTEGER→string
    /// handle: operand `a` is a NaN-boxed string handle, operand `b` a plain `i64`.
    pub const STK_STR_SUBSTR2: u16 = 0x001B;
    /// `$s x $n` string-repeat. Binary string+INTEGER→string handle (see `STK_STR_SUBSTR2`).
    pub const STK_STR_REPEAT: u16 = 0x001C;
    /// `substr($s, $off, $len)` (3-arg) — byte-offset substring. string + INTEGER +
    /// INTEGER → string handle: operand `a` is a NaN-boxed string handle, `b`/`c` plain
    /// `i64`s (see `STK_STR_SUBSTR2`).
    pub const STK_STR_SUBSTR3: u16 = 0x001D;

    /// `chr` — the one-character string for an integer codepoint. Unlike the other
    /// `STK_STR_*` ops its operand is an **integer** (not a string handle), but like
    /// `uc`/`concat` its result is the raw bits of a freshly allocated *owned*
    /// `StrykeValue::string` (reconstructed via `from_raw_bits`). So the operand is
    /// marshaled as an unboxed `i64` (the integer path), while the result is an owned
    /// string handle — its own int→string eligibility class.
    pub const STK_STR_CHR: u16 = 0x0017;

    /// `LoadConst(idx)` runtime resolution — read strykelang chunk constant `idx`
    /// from the thread-local pool set by [`super::run_linear_segment`] (via
    /// [`super::with_load_const_ctx`]) and return its raw bits as an *owned*
    /// handle (shallow-cloned, then `mem::forget`-ed). Disk-cache safe: the chunk
    /// hashes the constant *index* and the helper id (both stable across
    /// processes), never the per-process heap-pointer bits of the constant
    /// itself. Operand is the constant index, passed as a plain `i64` via
    /// [`LoadInt`](fusevm::Op::LoadInt); result is a NaN-boxed [`StrykeValue`]
    /// handle the downstream ext op consumes via `from_raw_bits`.
    pub const STK_VAL_LOAD_CONST: u16 = 0x0024;

    /// `index($s, $sub)` (2-arg) — byte offset of first occurrence, or -1. Binary
    /// string→int (two NaN-boxed string handle operands, `i64` result), like the
    /// comparison ops, so it stays on the integer block JIT + on-disk cache.
    pub const STK_STR_INDEX: u16 = 0x0018;
    /// `rindex($s, $sub)` (2-arg) — byte offset of *last* occurrence, or -1. Binary
    /// string→int (see `STK_STR_INDEX`).
    pub const STK_STR_RINDEX: u16 = 0x0019;

    /// First ID reserved for strykelang; frontends must keep their blocks disjoint.
    pub const STK_ID_BASE: u16 = STK_REGEX_MATCH;
}

/// `PushFrame` + `slot_count` × (`LoadInt` + `SetSlot`) — the preamble emitted
/// before a translated body so fusevm slots start from the marshaled values.
/// When `seed_slots` is false (string-comparison segments) the marshaled slot
/// values are supplied to the block JIT through its runtime `slots` buffer rather
/// than baked into the chunk, so the only preamble op is `PushFrame`. This keeps
/// the chunk's `op_hash` independent of the (per-run, non-deterministic) operand
/// pointer handles, which is what makes those chunks safely disk-cacheable.
#[inline]
fn preamble_len(slot_count: usize, seed_slots: bool) -> usize {
    if seed_slots {
        1 + slot_count * 2
    } else {
        1
    }
}

/// Perl/strykelang *floored* modulo: the result takes the sign of the divisor
/// (`-7 % 3 == 2`), unlike Rust/fusevm's truncated `%`. `b ∈ {0, -1}` is guarded
/// to avoid a divide trap; both yield `0` (mathematically for `-1`, and a safe
/// non-trapping placeholder for the erroneous `% 0` path, which the strykelang
/// interpreter rejects separately on the non-JIT path).
#[inline]
pub(crate) fn floored_mod(a: i64, b: i64) -> i64 {
    if b == 0 || b == -1 {
        return 0;
    }
    let r = a % b;
    if r != 0 && ((r ^ b) < 0) {
        r + b
    } else {
        r
    }
}

/// Reconstruct a borrowed view of a `StrykeValue` from its raw NaN-boxed bits
/// (passed across the JIT/interpreter boundary as an i64 slot handle) **without
/// taking ownership**: the value is owned by the caller's scope, so it must not
/// be dropped here (that would decrement the shared `Arc`). `ManuallyDrop`
/// guarantees no drop runs.
#[inline]
unsafe fn sv_borrow(bits: i64) -> std::mem::ManuallyDrop<StrykeValue> {
    std::mem::ManuallyDrop::new(StrykeValue(bits as u64))
}

/// Shared comparator for every `STK_STR_*` op. Reconstructs both operands from
/// their NaN-boxed bits and defers to strykelang's native string comparison so
/// grapheme/Unicode and stringification semantics match the interpreter exactly.
/// Returns 0/1 for the boolean ops and -1/0/1 for `cmp`.
#[inline]
fn stryke_str_compare_op(ext_id: u16, a_bits: i64, b_bits: i64) -> i64 {
    use std::cmp::Ordering;
    let (a, b) = unsafe { (sv_borrow(a_bits), sv_borrow(b_bits)) };
    match ext_id {
        ext_ops::STK_STR_EQ => a.str_eq(&b) as i64,
        ext_ops::STK_STR_NE => (!a.str_eq(&b)) as i64,
        ext_ops::STK_STR_LT => (a.str_cmp(&b) == Ordering::Less) as i64,
        ext_ops::STK_STR_GT => (a.str_cmp(&b) == Ordering::Greater) as i64,
        ext_ops::STK_STR_LE => {
            matches!(a.str_cmp(&b), Ordering::Less | Ordering::Equal) as i64
        }
        ext_ops::STK_STR_GE => {
            matches!(a.str_cmp(&b), Ordering::Greater | Ordering::Equal) as i64
        }
        ext_ops::STK_STR_CMP => match a.str_cmp(&b) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        },
        _ => 0,
    }
}

macro_rules! stryke_str_helper {
    ($name:ident, $id:expr) => {
        /// `extern "C"` host helper invoked from JIT-compiled code via
        /// `ExtJitCtx::call_host`. ABI: `(i64 a_bits, i64 b_bits) -> i64`.
        extern "C" fn $name(a: i64, b: i64) -> i64 {
            stryke_str_compare_op($id, a, b)
        }
    };
}
stryke_str_helper!(stryke_h_str_eq, ext_ops::STK_STR_EQ);
stryke_str_helper!(stryke_h_str_ne, ext_ops::STK_STR_NE);
stryke_str_helper!(stryke_h_str_lt, ext_ops::STK_STR_LT);
stryke_str_helper!(stryke_h_str_gt, ext_ops::STK_STR_GT);
stryke_str_helper!(stryke_h_str_le, ext_ops::STK_STR_LE);
stryke_str_helper!(stryke_h_str_ge, ext_ops::STK_STR_GE);
stryke_str_helper!(stryke_h_str_cmp, ext_ops::STK_STR_CMP);

/// Shared computation for the binary string→int search ops `index`/`rindex` (2-arg
/// form). Reconstructs both operands from their NaN-boxed bits and defers to
/// strykelang's native [`StrykeValue::index_value`] / [`StrykeValue::rindex_value`]
/// so stringification and byte-offset semantics match the interpreter exactly.
#[inline]
fn stryke_str_index_op(ext_id: u16, a_bits: i64, b_bits: i64) -> i64 {
    let (a, b) = unsafe { (sv_borrow(a_bits), sv_borrow(b_bits)) };
    match ext_id {
        ext_ops::STK_STR_INDEX => a.index_value(&b),
        ext_ops::STK_STR_RINDEX => a.rindex_value(&b),
        _ => -1,
    }
}

/// `extern "C"` host helper for `STK_STR_INDEX`. ABI matches the compare helpers
/// (`(i64 a_bits, i64 b_bits) -> i64`) so it shares the binary string machinery.
extern "C" fn stryke_h_str_index(a: i64, b: i64) -> i64 {
    stryke_str_index_op(ext_ops::STK_STR_INDEX, a, b)
}
/// `extern "C"` host helper for `STK_STR_RINDEX`.
extern "C" fn stryke_h_str_rindex(a: i64, b: i64) -> i64 {
    stryke_str_index_op(ext_ops::STK_STR_RINDEX, a, b)
}

/// True for the binary string→int search ops (`index`/`rindex`), whose operands are
/// string handles and whose result is an `i64` offset.
#[inline]
fn is_stryke_str_index_ext(ext_id: u16) -> bool {
    matches!(ext_id, ext_ops::STK_STR_INDEX | ext_ops::STK_STR_RINDEX)
}

/// Concatenate two operands (`$a . $b`), returning the raw bits of a **newly
/// allocated, owned** `StrykeValue::string`. The two operands are borrowed
/// (never dropped — they stay owned by the caller's slots or constants pool);
/// the result's `Arc` ownership is transferred into the returned handle via
/// `mem::forget`, so the caller must reconstitute exactly one owning
/// `StrykeValue` from these bits (`StrykeValue::from_raw_bits`) — which
/// [`run_linear_segment`] does.
///
/// Uses [`StrykeValue::append_to`] (byte-identical to `push_str` for plain
/// strings — the seeded-slot fast path — and produces the same Perl-style
/// stringification the interpreter would for non-string operands like integer
/// or float constants reached via the `LoadConst → STK_VAL_LOAD_CONST`
/// translation). Still does NOT trigger `use overload '""'` (slot operands are
/// gated by `is_string_like`, and LoadConst constants are compile-time
/// literals that can't be blessed), so semantics remain a pure byte-level join.
#[inline]
fn stryke_str_concat_op(a_bits: i64, b_bits: i64) -> i64 {
    let (a, b) = unsafe { (sv_borrow(a_bits), sv_borrow(b_bits)) };
    let mut s = String::new();
    a.append_to(&mut s);
    b.append_to(&mut s);
    let new = StrykeValue::string(s);
    let bits = new.raw_bits() as i64;
    // Transfer the new value's Arc ownership into the returned handle; the caller
    // reconstructs a single owning StrykeValue, so there is no leak/double-free.
    std::mem::forget(new);
    bits
}

/// `extern "C"` host helper for `STK_STR_CONCAT`. ABI matches the compare helpers
/// (`(i64, i64) -> i64`) so it shares the same `call_host`/relocation machinery.
extern "C" fn stryke_h_str_concat(a: i64, b: i64) -> i64 {
    stryke_str_concat_op(a, b)
}

/// `crypt($plaintext, $salt)`: pure-Rust `perl_crypt`. Both operands are
/// borrowed NaN-boxed string handles; the result is an owned string handle
/// (same `mem::forget` transfer as `STK_STR_CONCAT`).
#[inline]
fn stryke_str_crypt_op(a_bits: i64, b_bits: i64) -> i64 {
    let (a, b) = unsafe { (sv_borrow(a_bits), sv_borrow(b_bits)) };
    forget_string_bits(crate::crypt_util::perl_crypt(
        &a.as_str().unwrap_or_default(),
        &b.as_str().unwrap_or_default(),
    ))
}
extern "C" fn stryke_h_str_crypt(a: i64, b: i64) -> i64 {
    stryke_str_crypt_op(a, b)
}

/// `substr($s, $off)` (2-arg): operand `a_bits` is a borrowed NaN-boxed string handle,
/// `off` a plain `i64`. Returns the raw bits of a freshly allocated owned string handle
/// (see [`StrykeValue::substr2_value`]).
#[inline]
fn stryke_str_substr2_op(a_bits: i64, off: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.substr2_value(off))
}
extern "C" fn stryke_h_str_substr2(a: i64, off: i64) -> i64 {
    stryke_str_substr2_op(a, off)
}

/// `$s x $n` string-repeat: operand `a_bits` is a borrowed NaN-boxed string handle, `n`
/// a plain `i64`. Returns the raw bits of an owned string handle (see
/// [`StrykeValue::repeat_value`]).
#[inline]
fn stryke_str_repeat_op(a_bits: i64, n: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.repeat_value(n))
}
extern "C" fn stryke_h_str_repeat(a: i64, n: i64) -> i64 {
    stryke_str_repeat_op(a, n)
}

/// `substr($s, $off, $len)` (3-arg): operand `a_bits` is a borrowed NaN-boxed string
/// handle, `off`/`len` plain `i64`s. Returns the raw bits of an owned string handle
/// (see [`StrykeValue::substr3_value`]).
#[inline]
fn stryke_str_substr3_op(a_bits: i64, off: i64, len: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.substr3_value(off, len))
}
extern "C" fn stryke_h_str_substr3(a: i64, off: i64, len: i64) -> i64 {
    stryke_str_substr3_op(a, off, len)
}

thread_local! {
    /// Mirror of the interpreter's `utf8_pragma`, refreshed at each fusevm block
    /// dispatch (see [`set_utf8_pragma`]). `length`'s host helper runs on the same
    /// thread as the interpreter that set it, so it reads the *live* pragma value;
    /// keeping it out of the chunk leaves the chunk operand/pragma-independent
    /// (hence disk-cacheable) while still matching the interpreter across `use utf8`
    /// / `no utf8` toggles, which flip the pragma at runtime.
    static STRYKE_UTF8_PRAGMA: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Refresh the thread-local `utf8` pragma the `length` helper consults. Called by
/// the interpreter immediately before dispatching a segment to the fusevm block
/// JIT, so a JIT-computed `length` matches the interpreter under the current pragma.
#[inline]
pub(crate) fn set_utf8_pragma(on: bool) {
    STRYKE_UTF8_PRAGMA.with(|p| p.set(on));
}

/// Stable helper-symbol name for `STK_STR_LEN`, hashed into a JIT helper id and
/// registered with arity 1 (unary) — distinct from the binary string helpers.
const STRYKE_STR_LEN_SYM: &str = "stryke_str_len";

/// `length($s)`: borrow the operand `StrykeValue` from its NaN-boxed bits and
/// return its length using the same [`StrykeValue::length_value`] the interpreter
/// uses, under the live thread-local `utf8` pragma. Eligibility guarantees the
/// operand is a plain string, so this never drops/allocates.
#[inline]
fn stryke_str_len_op(a_bits: i64) -> i64 {
    let a = unsafe { sv_borrow(a_bits) };
    a.length_value(STRYKE_UTF8_PRAGMA.with(|p| p.get()))
}

/// `extern "C"` host helper for `STK_STR_LEN`. ABI: `(i64 handle) -> i64` (unary).
extern "C" fn stryke_h_str_len(a: i64) -> i64 {
    stryke_str_len_op(a)
}

/// `ord($s)`: first char's codepoint (see [`StrykeValue::ord_value`]).
#[inline]
fn stryke_str_ord_op(a_bits: i64) -> i64 {
    unsafe { sv_borrow(a_bits) }.ord_value()
}
extern "C" fn stryke_h_str_ord(a: i64) -> i64 {
    stryke_str_ord_op(a)
}

/// `hex($s)`: parse hexadecimal (see [`StrykeValue::hex_value`]).
#[inline]
fn stryke_str_hex_op(a_bits: i64) -> i64 {
    unsafe { sv_borrow(a_bits) }.hex_value()
}
extern "C" fn stryke_h_str_hex(a: i64) -> i64 {
    stryke_str_hex_op(a)
}

/// `oct($s)`: parse octal/hex/binary (see [`StrykeValue::oct_value`]).
#[inline]
fn stryke_str_oct_op(a_bits: i64) -> i64 {
    unsafe { sv_borrow(a_bits) }.oct_value()
}
extern "C" fn stryke_h_str_oct(a: i64) -> i64 {
    stryke_str_oct_op(a)
}

/// Table of the unary string ops (`(i64 handle) -> i64`): `length`/`ord`/`hex`/`oct`.
/// Each maps its `STK_STR_*` Extended id to the stable host-helper symbol (hashed
/// into a JIT helper id and registered with arity 1). Distinct from the binary
/// `STRYKE_STR_HELPERS` table because the ABI differs (one operand, not two).
const STRYKE_STR_UNARY_HELPERS: &[(u16, &str, extern "C" fn(i64) -> i64)] = &[
    (ext_ops::STK_STR_LEN, STRYKE_STR_LEN_SYM, stryke_h_str_len),
    (ext_ops::STK_STR_ORD, "stryke_str_ord", stryke_h_str_ord),
    (ext_ops::STK_STR_HEX, "stryke_str_hex", stryke_h_str_hex),
    (ext_ops::STK_STR_OCT, "stryke_str_oct", stryke_h_str_oct),
];

/// True for any unary string Extended id (`length`/`ord`/`hex`/`oct` →int, or
/// `uc`/`lc`/`ucfirst`/`lcfirst` →string handle). Both share the `(i64) -> i64`
/// host-helper ABI and the single-operand emit/dispatch path.
#[inline]
fn is_stryke_str_unary_ext(ext_id: u16) -> bool {
    STRYKE_STR_UNARY_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
        || is_stryke_str_unary_str_ext(ext_id)
        || is_stryke_int_str_ext(ext_id)
}

/// The registered JIT helper id for a unary string Extended op, if any.
#[inline]
fn stryke_str_unary_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_STR_UNARY_HELPERS
        .iter()
        .chain(STRYKE_STR_UNARY_STR_HELPERS.iter())
        .chain(STRYKE_INT_STR_HELPERS.iter())
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// Interpreter-side computation for a unary string Extended op (mirrors the JIT
/// host helpers so the cold path agrees with JITted code).
#[inline]
fn stryke_str_unary_op(ext_id: u16, a_bits: i64) -> i64 {
    match ext_id {
        ext_ops::STK_STR_LEN => stryke_str_len_op(a_bits),
        ext_ops::STK_STR_ORD => stryke_str_ord_op(a_bits),
        ext_ops::STK_STR_HEX => stryke_str_hex_op(a_bits),
        ext_ops::STK_STR_OCT => stryke_str_oct_op(a_bits),
        _ => stryke_str_unary_str_op(ext_id, a_bits),
    }
}

thread_local! {
    /// Pointer + length of the strykelang chunk-constants slice the
    /// currently-executing `run_linear_segment` call is running against.
    /// `with_load_const_ctx` sets this before invoking `try_run_block` and
    /// clears it on drop, so [`stryke_h_val_load_const`] can resolve a
    /// `LoadConst(idx)` operand to the right `StrykeValue` at runtime — without
    /// baking the per-process heap-pointer bits into the chunk (which would
    /// break the on-disk cache key by making `op_hash` process-specific).
    ///
    /// Null when no segment is executing; a non-null pointer is valid only for
    /// the duration of the `with_load_const_ctx` scope that set it (the chunk
    /// runs inside that scope, so the slice borrow outlives every call to the
    /// helper).
    static LOAD_CONST_CTX: std::cell::Cell<(*const StrykeValue, usize)> =
        const { std::cell::Cell::new((std::ptr::null(), 0)) };
}

/// RAII guard that publishes a `&[StrykeValue]` constants slice to
/// [`LOAD_CONST_CTX`] for the duration of the guard's lifetime, restoring the
/// prior value on drop (so nested `run_linear_segment` calls correctly stack).
struct LoadConstCtxGuard {
    prev: (*const StrykeValue, usize),
}

impl LoadConstCtxGuard {
    fn new(constants: &[StrykeValue]) -> Self {
        let prev = LOAD_CONST_CTX.with(|c| c.replace((constants.as_ptr(), constants.len())));
        LoadConstCtxGuard { prev }
    }
}

impl Drop for LoadConstCtxGuard {
    fn drop(&mut self) {
        LOAD_CONST_CTX.with(|c| c.set(self.prev));
    }
}

/// `STK_VAL_LOAD_CONST` interpreter/host-helper body. Resolves a `LoadConst(idx)`
/// operand by looking the constant up in the thread-local pool published by the
/// current [`LoadConstCtxGuard`] and returning its raw bits *as a borrowed view*
/// — no `shallow_clone`, no `mem::forget`. The downstream ext op (concat /
/// compare / etc.) reads the bits via [`sv_borrow`] (`ManuallyDrop`), which
/// never decrements the `Arc`; ownership stays with the chunk-constants pool
/// the [`LoadConstCtxGuard`] is currently publishing.
///
/// This matches the seeded-slot operand contract exactly — both kinds of input
/// to a chunk are *borrowed* views into something the caller owns and keeps
/// alive across the JIT call. Any future consumer that *does* drain its
/// operand (e.g. a hypothetical sprintf helper using `from_raw_bits`) must
/// either go through a different ext op or `shallow_clone` the borrowed bits
/// before consuming, otherwise it would over-release the constants pool entry.
///
/// Out-of-range indices and an unset context both yield bare `UNDEF` bits
/// (still borrow-only — `UNDEF` is a tagged NaN, not heap-backed) rather than
/// panicking; either case indicates a bridge bug that the interpreter fallback
/// can still observe without crashing the process.
#[inline]
fn stryke_val_load_const_op(idx_bits: i64) -> i64 {
    LOAD_CONST_CTX.with(|c| {
        let (ptr, len) = c.get();
        let idx = idx_bits as usize;
        if ptr.is_null() || idx >= len {
            return StrykeValue::UNDEF.raw_bits() as i64;
        }
        // SAFETY: `LoadConstCtxGuard::new` set `ptr`/`len` from a `&[StrykeValue]`
        // borrow that outlives every helper call dispatched from inside the same
        // `try_run_block`; the guard restores the prior pointer on drop so any
        // nested segment correctly sees its own pool.
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        slice[idx].raw_bits() as i64
    })
}
extern "C" fn stryke_h_val_load_const(idx: i64) -> i64 {
    stryke_val_load_const_op(idx)
}

/// Build a freshly-allocated, *owned* `StrykeValue::string` from `s` and return its
/// raw NaN-boxed bits, transferring `Arc` ownership into the handle via `mem::forget`
/// (identical contract to [`stryke_str_concat_op`]): the caller reconstitutes exactly
/// one owning `StrykeValue` via `from_raw_bits`, so there is no leak/double-free.
#[inline]
fn forget_string_bits(s: String) -> i64 {
    let new = StrykeValue::string(s);
    let bits = new.raw_bits() as i64;
    std::mem::forget(new);
    bits
}

/// `uc($s)`: borrow the operand and return an owned upper-cased string handle, using
/// the same [`StrykeValue::uc_value`] the interpreter uses. Eligibility guarantees a
/// plain-string operand.
#[inline]
fn stryke_str_uc_op(a_bits: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.uc_value())
}
extern "C" fn stryke_h_str_uc(a: i64) -> i64 {
    stryke_str_uc_op(a)
}

/// `lc($s)`: owned lower-cased string handle (see [`StrykeValue::lc_value`]).
#[inline]
fn stryke_str_lc_op(a_bits: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.lc_value())
}
extern "C" fn stryke_h_str_lc(a: i64) -> i64 {
    stryke_str_lc_op(a)
}

/// `ucfirst($s)`: owned handle, first char upper-cased (see [`StrykeValue::ucfirst_value`]).
#[inline]
fn stryke_str_ucfirst_op(a_bits: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.ucfirst_value())
}
extern "C" fn stryke_h_str_ucfirst(a: i64) -> i64 {
    stryke_str_ucfirst_op(a)
}

/// `lcfirst($s)`: owned handle, first char lower-cased (see [`StrykeValue::lcfirst_value`]).
#[inline]
fn stryke_str_lcfirst_op(a_bits: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.lcfirst_value())
}
extern "C" fn stryke_h_str_lcfirst(a: i64) -> i64 {
    stryke_str_lcfirst_op(a)
}

/// `fc($s)`: owned case-folded string handle (see [`StrykeValue::fc_value`]).
#[inline]
fn stryke_str_fc_op(a_bits: i64) -> i64 {
    forget_string_bits(unsafe { sv_borrow(a_bits) }.fc_value())
}
extern "C" fn stryke_h_str_fc(a: i64) -> i64 {
    stryke_str_fc_op(a)
}

/// `quotemeta($s)`: owned handle wrapping `perl_quotemeta` (Perl `\Q…\E`).
/// Backslash-escapes every non-word character; pure functional.
#[inline]
fn stryke_str_quotemeta_op(a_bits: i64) -> i64 {
    let v = unsafe { sv_borrow(a_bits) };
    forget_string_bits(crate::perl_regex::perl_quotemeta(&v.to_string()))
}
extern "C" fn stryke_h_str_quotemeta(a: i64) -> i64 {
    stryke_str_quotemeta_op(a)
}

/// `reverse($s)` (scalar form): owned handle of `$s`'s chars reversed. The
/// list form (`reverse(@arr)`) returns an array and is NOT routed here — the
/// detector only emits this op when the operand is a scalar string.
#[inline]
fn stryke_str_reverse_op(a_bits: i64) -> i64 {
    let v = unsafe { sv_borrow(a_bits) };
    forget_string_bits(v.to_string().chars().rev().collect::<String>())
}
extern "C" fn stryke_h_str_reverse(a: i64) -> i64 {
    stryke_str_reverse_op(a)
}

/// `defined($x)`: returns 1 if `$x` is anything other than UNDEF, else 0. The
/// operand is a raw NaN-boxed StrykeValue handle of ANY type (string, integer,
/// float, hash ref, undef, …). Unlike `STK_STR_*` helpers there's no operand
/// type gate at the call site — the helper accepts every kind.
#[inline]
fn stryke_val_defined_op(a_bits: i64) -> i64 {
    let v = unsafe { sv_borrow(a_bits) };
    if v.is_undef() { 0 } else { 1 }
}
extern "C" fn stryke_h_val_defined(a: i64) -> i64 {
    stryke_val_defined_op(a)
}

/// `ref($x)`: owned-string handle wrapping the Perl-style type name of `$x`
/// (delegates to [`StrykeValue::ref_type`], whose result is itself a
/// `StrykeValue::string`). Operand can be ANY type — no `is_string_like` gate.
#[inline]
fn stryke_val_ref_op(a_bits: i64) -> i64 {
    let v = unsafe { sv_borrow(a_bits) };
    let result = v.ref_type();
    forget_string_bits(result.as_str().unwrap_or_default())
}
extern "C" fn stryke_h_val_ref(a: i64) -> i64 {
    stryke_val_ref_op(a)
}

/// `round($x)` (1-arg): ties AWAY from zero, returns Int. Coerces via
/// `to_number()` so any input type works (UNDEF→0, string→numeric-parse, etc).
/// Avoids a fusevm Op::RoundAway by doing the rounding in the host helper.
#[inline]
fn stryke_val_round_op(a_bits: i64) -> i64 {
    let v = unsafe { sv_borrow(a_bits) };
    v.to_number().round() as i64
}
extern "C" fn stryke_h_val_round(a: i64) -> i64 {
    stryke_val_round_op(a)
}

/// Table of unary any-value→int ext ops. Same `(i64) -> i64` ABI as the unary
/// string→int helpers, but at the call site the seeder skips the
/// `is_string_like` gate so the operand can be any type.
const STRYKE_VAL_UNARY_INT_HELPERS: &[(u16, &str, extern "C" fn(i64) -> i64)] = &[
    (ext_ops::STK_VAL_DEFINED, "stryke_val_defined", stryke_h_val_defined),
    (ext_ops::STK_VAL_ROUND, "stryke_val_round", stryke_h_val_round),
];

/// Table of unary any-value→**string handle** ext ops. Same `(i64) -> i64` ABI as
/// `STRYKE_VAL_UNARY_INT_HELPERS`, but the returned `i64` is the raw bits of an
/// owned `StrykeValue::string` (reconstructed via `from_raw_bits`, like
/// `STK_STR_CONCAT`), not a plain integer. Caller bypasses `is_string_like`.
const STRYKE_VAL_UNARY_STR_HELPERS: &[(u16, &str, extern "C" fn(i64) -> i64)] = &[
    (ext_ops::STK_VAL_REF, "stryke_val_ref", stryke_h_val_ref),
];

/// True for any-value unary→**string** ext id (currently just `ref`).
#[inline]
fn is_stryke_val_unary_str_ext(ext_id: u16) -> bool {
    STRYKE_VAL_UNARY_STR_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// Registered JIT helper id for an any-value unary→string ext op.
#[inline]
fn stryke_val_unary_str_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_VAL_UNARY_STR_HELPERS
        .iter()
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// Interpreter-side dispatch for an any-value unary→string ext op.
#[inline]
fn stryke_val_unary_str_op(ext_id: u16, a_bits: i64) -> i64 {
    match ext_id {
        ext_ops::STK_VAL_REF => stryke_val_ref_op(a_bits),
        _ => 0,
    }
}

/// Maps a unary any-value→string builtin call to its `STK_VAL_*` ext id.
#[inline]
fn unary_any_str_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    let Op::CallBuiltin(id, 1) = op else { return None; };
    if *id == BuiltinId::Ref as u16 {
        Some(ext_ops::STK_VAL_REF)
    } else {
        None
    }
}

/// True when `seg` is `[GetScalarSlot, CallBuiltin(ref, 1)]` — a single
/// any-value unary→string builtin. Same shape as the int variant; only
/// the result type differs (handle vs. integer).
pub(crate) fn segment_is_any_value_unary_str_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(seg, [Op::GetScalarSlot(_), u] if unary_any_str_ext_op(u).is_some())
}

/// Registered helper name + JIT helper id for `STK_VAL_LOAD_CONST`. Kept as a
/// pair of free functions (rather than slotted into one of the unary tables)
/// because the operand semantics are different: the input `i64` is a *constant
/// index* (not a NaN-boxed handle, not an integer-context value), and the
/// helper resolves it against a thread-local context rather than computing
/// purely from the operand. The result is an owned `StrykeValue` handle
/// (any kind: int, float, string, …) that the downstream ext op consumes via
/// `from_raw_bits`.
const STRYKE_VAL_LOAD_CONST_NAME: &str = "stryke_val_load_const";

#[inline]
fn is_stryke_val_load_const_ext(ext_id: u16) -> bool {
    ext_id == ext_ops::STK_VAL_LOAD_CONST
}

#[inline]
fn stryke_val_load_const_helper_id() -> u32 {
    fusevm::jit::jit_helper_id(STRYKE_VAL_LOAD_CONST_NAME)
}

/// True for an any-value unary→int ext id (currently just `defined`).
#[inline]
fn is_stryke_val_unary_int_ext(ext_id: u16) -> bool {
    STRYKE_VAL_UNARY_INT_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// Registered JIT helper id for an any-value unary→int ext op.
#[inline]
fn stryke_val_unary_int_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_VAL_UNARY_INT_HELPERS
        .iter()
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// Interpreter-side dispatch for an any-value unary→int ext op.
#[inline]
fn stryke_val_unary_int_op(ext_id: u16, a_bits: i64) -> i64 {
    match ext_id {
        ext_ops::STK_VAL_DEFINED => stryke_val_defined_op(a_bits),
        ext_ops::STK_VAL_ROUND => stryke_val_round_op(a_bits),
        _ => 0,
    }
}

/// Maps a unary any-value→int builtin call to its `STK_VAL_*` ext id, if any.
/// Currently just `defined`.
#[inline]
fn unary_any_int_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    let Op::CallBuiltin(id, 1) = op else { return None; };
    if *id == BuiltinId::Defined as u16 {
        Some(ext_ops::STK_VAL_DEFINED)
    } else if *id == BuiltinId::Round as u16 {
        Some(ext_ops::STK_VAL_ROUND)
    } else {
        None
    }
}

/// True when `seg` is `[GetScalarSlot, CallBuiltin(defined, 1)]` — a single
/// any-value unary→int builtin. Same shape as the string unary segments but
/// the operand can be any type (the caller's seeder MUST NOT gate on
/// `is_string_like`).
pub(crate) fn segment_is_any_value_unary_int_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(seg, [Op::GetScalarSlot(_), u] if unary_any_int_ext_op(u).is_some())
}

/// Table of the unary string→**string** ops (`(i64 handle) -> i64 handle`):
/// `uc`/`lc`/`ucfirst`/`lcfirst`. Same `(i64) -> i64` ABI as the unary string→int
/// table, but the returned `i64` is the raw bits of a freshly allocated *owned*
/// `StrykeValue::string` (reconstructed via `from_raw_bits`, like `STK_STR_CONCAT`),
/// not a plain integer. Kept in its own table so [`run_linear_segment`] knows to
/// reconstruct an owning handle rather than box an integer.
const STRYKE_STR_UNARY_STR_HELPERS: &[(u16, &str, extern "C" fn(i64) -> i64)] = &[
    (ext_ops::STK_STR_UC, "stryke_str_uc", stryke_h_str_uc),
    (ext_ops::STK_STR_LC, "stryke_str_lc", stryke_h_str_lc),
    (ext_ops::STK_STR_UCFIRST, "stryke_str_ucfirst", stryke_h_str_ucfirst),
    (ext_ops::STK_STR_LCFIRST, "stryke_str_lcfirst", stryke_h_str_lcfirst),
    (ext_ops::STK_STR_FC, "stryke_str_fc", stryke_h_str_fc),
    (ext_ops::STK_STR_QUOTEMETA, "stryke_str_quotemeta", stryke_h_str_quotemeta),
    (ext_ops::STK_STR_REVERSE, "stryke_str_reverse", stryke_h_str_reverse),
];

/// True for a unary string→string Extended id (`uc`/`lc`/`ucfirst`/`lcfirst`), whose
/// result is an owned string handle rather than an integer.
#[inline]
fn is_stryke_str_unary_str_ext(ext_id: u16) -> bool {
    STRYKE_STR_UNARY_STR_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// Interpreter-side computation for a unary string→string Extended op; returns the
/// raw bits of an owned string handle (mirrors the JIT host helpers).
#[inline]
fn stryke_str_unary_str_op(ext_id: u16, a_bits: i64) -> i64 {
    match ext_id {
        ext_ops::STK_STR_UC => stryke_str_uc_op(a_bits),
        ext_ops::STK_STR_LC => stryke_str_lc_op(a_bits),
        ext_ops::STK_STR_UCFIRST => stryke_str_ucfirst_op(a_bits),
        ext_ops::STK_STR_LCFIRST => stryke_str_lcfirst_op(a_bits),
        ext_ops::STK_STR_FC => stryke_str_fc_op(a_bits),
        ext_ops::STK_STR_QUOTEMETA => stryke_str_quotemeta_op(a_bits),
        ext_ops::STK_STR_REVERSE => stryke_str_reverse_op(a_bits),
        ext_ops::STK_STR_CHR => stryke_str_chr_op(a_bits),
        _ => 0,
    }
}

/// `chr($n)`: build the one-character owned string for integer codepoint `n` (the
/// operand is a plain `i64`, *not* a NaN-boxed handle), via the same
/// [`crate::value::chr_from_codepoint`] the interpreter uses. Returns an owned string
/// handle (raw bits of a `mem::forget`-ed `StrykeValue::string`).
#[inline]
fn stryke_str_chr_op(n: i64) -> i64 {
    forget_string_bits(crate::value::chr_from_codepoint(n))
}
extern "C" fn stryke_h_str_chr(n: i64) -> i64 {
    stryke_str_chr_op(n)
}

/// Table of int→string ops (`(i64 codepoint) -> i64 string handle`): currently just
/// `chr`. The `(i64) -> i64` helper ABI matches the unary tables, so emit/dispatch
/// reuse the single-operand path; the distinction is the *operand* is an integer (so
/// it is marshaled unboxed, via the integer path) rather than a string handle.
const STRYKE_INT_STR_HELPERS: &[(u16, &str, extern "C" fn(i64) -> i64)] = &[
    (ext_ops::STK_STR_CHR, "stryke_str_chr", stryke_h_str_chr),
];

/// True for an int→string Extended id (`chr`), whose operand is an integer and whose
/// result is an owned string handle.
#[inline]
fn is_stryke_int_str_ext(ext_id: u16) -> bool {
    STRYKE_INT_STR_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// Table of binary string+INTEGER→string ops (`(i64 str_handle, i64 int) -> i64 string
/// handle`): `substr($s,$off)` (2-arg) and the `$s x $n` repeat operator. Unlike the
/// other binary string ops, the two operands marshal with DIFFERENT kinds — the first
/// as a NaN-boxed string handle, the second as a plain unboxed integer — so the caller
/// (`vm.rs`) marshals these slots per-operand (see `string_int_slot_kinds`). The result
/// is an owned string handle reconstructed via `from_raw_bits`.
const STRYKE_STR_INT_STR_HELPERS: &[(u16, &str, extern "C" fn(i64, i64) -> i64)] = &[
    (ext_ops::STK_STR_SUBSTR2, "stryke_str_substr2", stryke_h_str_substr2),
    (ext_ops::STK_STR_REPEAT, "stryke_str_repeat", stryke_h_str_repeat),
];

/// True for a binary string+integer→string Extended id (`substr` 2-arg, `x`-repeat).
#[inline]
fn is_stryke_str_int_str_ext(ext_id: u16) -> bool {
    STRYKE_STR_INT_STR_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// The registered JIT helper id for a binary string+integer→string op, if any.
#[inline]
fn stryke_str_int_str_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_STR_INT_STR_HELPERS
        .iter()
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// Interpreter-side computation for a binary string+integer→string Extended op; returns
/// the raw bits of an owned string handle (mirrors the JIT host helpers).
#[inline]
fn stryke_str_int_str_op(ext_id: u16, a_bits: i64, b: i64) -> i64 {
    match ext_id {
        ext_ops::STK_STR_SUBSTR2 => stryke_str_substr2_op(a_bits, b),
        ext_ops::STK_STR_REPEAT => stryke_str_repeat_op(a_bits, b),
        _ => 0,
    }
}

/// Table of ternary string+INTEGER+INTEGER→string ops (`(i64 str_handle, i64, i64) ->
/// i64 string handle`): currently just `substr($s,$off,$len)` (3-arg). Like the binary
/// mixed family, operand 0 marshals as a string handle and the rest as plain integers
/// (per-operand marshaling via `string_int_slot_kinds`); the result is an owned string
/// handle reconstructed via `from_raw_bits`.
const STRYKE_STR_INT2_STR_HELPERS: &[(u16, &str, extern "C" fn(i64, i64, i64) -> i64)] = &[
    (ext_ops::STK_STR_SUBSTR3, "stryke_str_substr3", stryke_h_str_substr3),
];

/// True for a ternary string+integer+integer→string Extended id (`substr` 3-arg).
#[inline]
fn is_stryke_str_int2_str_ext(ext_id: u16) -> bool {
    STRYKE_STR_INT2_STR_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// The registered JIT helper id for a ternary string+int+int→string op, if any.
#[inline]
fn stryke_str_int2_str_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_STR_INT2_STR_HELPERS
        .iter()
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// Interpreter-side computation for a ternary string+int+int→string Extended op; returns
/// the raw bits of an owned string handle (mirrors the JIT host helpers).
#[inline]
fn stryke_str_int2_str_op(ext_id: u16, a_bits: i64, b: i64, c: i64) -> i64 {
    match ext_id {
        ext_ops::STK_STR_SUBSTR3 => stryke_str_substr3_op(a_bits, b, c),
        _ => 0,
    }
}

/// Table mapping each string-compare `Extended` id to the stable host-helper
/// symbol name and its function pointer. The name is hashed into a JIT helper id
/// (`fusevm::jit::jit_helper_id`) so cached native code re-resolves the helper at
/// load time; the pointer is registered once via `register_jit_helper`.
const STRYKE_STR_HELPERS: &[(u16, &str, extern "C" fn(i64, i64) -> i64)] = &[
    (ext_ops::STK_STR_EQ, "stryke_str_eq", stryke_h_str_eq),
    (ext_ops::STK_STR_NE, "stryke_str_ne", stryke_h_str_ne),
    (ext_ops::STK_STR_LT, "stryke_str_lt", stryke_h_str_lt),
    (ext_ops::STK_STR_GT, "stryke_str_gt", stryke_h_str_gt),
    (ext_ops::STK_STR_LE, "stryke_str_le", stryke_h_str_le),
    (ext_ops::STK_STR_GE, "stryke_str_ge", stryke_h_str_ge),
    (ext_ops::STK_STR_CMP, "stryke_str_cmp", stryke_h_str_cmp),
    (ext_ops::STK_STR_CONCAT, "stryke_str_concat", stryke_h_str_concat),
    (ext_ops::STK_STR_INDEX, "stryke_str_index", stryke_h_str_index),
    (ext_ops::STK_STR_RINDEX, "stryke_str_rindex", stryke_h_str_rindex),
    (ext_ops::STK_STR_CRYPT, "stryke_str_crypt", stryke_h_str_crypt),
];

/// True for any binary `STK_STR_*` extension id (comparisons + concatenation +
/// `index`/`rindex`). Table-driven rather than a contiguous range because these ids
/// are interleaved with the unary/string-producing ops in the `STK_STR_*` space.
#[inline]
fn is_stryke_str_ext(ext_id: u16) -> bool {
    STRYKE_STR_HELPERS.iter().any(|(id, _, _)| *id == ext_id)
}

/// The registered JIT helper id for a string-compare extension op, if any.
#[inline]
fn stryke_str_helper_id(ext_id: u16) -> Option<u32> {
    STRYKE_STR_HELPERS
        .iter()
        .find(|(id, _, _)| *id == ext_id)
        .map(|(_, name, _)| fusevm::jit::jit_helper_id(name))
}

/// fusevm JIT extension that lowers strykelang's `Op::Mod` (re-emitted as
/// `fusevm::Op::Extended(STK_MOD_FLOOR, 0)`) to native floored-modulo code, plus
/// the `STK_STR_*` string-comparison ops (which call host helpers, kept cacheable
/// by fusevm's relocation-replay), keeping them on the block JIT + on-disk cache.
pub(crate) struct StrykeJitExt;
impl fusevm::jit::JitExtension for StrykeJitExt {
    fn can_jit(&self, ext_id: u16) -> bool {
        ext_id == ext_ops::STK_MOD_FLOOR
            || is_stryke_str_unary_ext(ext_id)
            || is_stryke_str_ext(ext_id)
            || is_stryke_str_int_str_ext(ext_id)
            || is_stryke_str_int2_str_ext(ext_id)
            || is_stryke_val_unary_int_ext(ext_id)
            || is_stryke_val_unary_str_ext(ext_id)
            || is_stryke_val_load_const_ext(ext_id)
    }
    fn op_count(&self) -> usize {
        // +1 for STK_MOD_FLOOR; +1 for STK_VAL_LOAD_CONST.
        2 + STRYKE_STR_HELPERS.len()
            + STRYKE_STR_UNARY_HELPERS.len()
            + STRYKE_STR_UNARY_STR_HELPERS.len()
            + STRYKE_INT_STR_HELPERS.len()
            + STRYKE_STR_INT_STR_HELPERS.len()
            + STRYKE_STR_INT2_STR_HELPERS.len()
            + STRYKE_VAL_UNARY_INT_HELPERS.len()
            + STRYKE_VAL_UNARY_STR_HELPERS.len()
    }
    fn name(&self) -> &str {
        "strykelang"
    }
    fn emit_extended(&self, ext_id: u16, _arg: u8, cx: &mut fusevm::jit::ExtJitCtx) -> bool {
        if is_stryke_str_unary_ext(ext_id) {
            // Unary string ops (length/ord/hex/oct): pop the single operand handle
            // and call the registered unary host helper. `call_host` records the
            // relocation so the chunk stays on-disk cacheable.
            let Some(helper_id) = stryke_str_unary_helper_id(ext_id) else {
                return false;
            };
            let Some(a) = cx.pop_i64() else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if is_stryke_val_load_const_ext(ext_id) {
            // `LoadConst(idx)` runtime resolution: pop the constant index
            // (previously emitted by [`translate_op_into`] via `LoadInt(idx)`)
            // and call the registered host helper, which reads the constant
            // from the thread-local pool set by [`LoadConstCtxGuard`] and
            // returns the raw bits of an owned handle. Same single-`(i64) -> i64`
            // shape as the unary string helpers, so the codegen is identical.
            let helper_id = stryke_val_load_const_helper_id();
            let Some(a) = cx.pop_i64() else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if is_stryke_val_unary_int_ext(ext_id) {
            // Unary any-value→int ops (`defined`): same shape as unary string ops
            // above, but the caller's seeder MUST NOT gate on `is_string_like`
            // because the operand can be UNDEF.
            let Some(helper_id) = stryke_val_unary_int_helper_id(ext_id) else {
                return false;
            };
            let Some(a) = cx.pop_i64() else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if is_stryke_val_unary_str_ext(ext_id) {
            // Unary any-value→string handle ops (`ref`): same shape as the
            // val-unary-int branch, but the returned `i64` is the raw bits of an
            // owned string handle (reconstructed via `from_raw_bits` by the caller).
            let Some(helper_id) = stryke_val_unary_str_helper_id(ext_id) else {
                return false;
            };
            let Some(a) = cx.pop_i64() else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if is_stryke_str_ext(ext_id) {
            // String compares: pop the two operand handles (raw StrykeValue bits)
            // and emit a call to the registered host helper, which defers to
            // strykelang's native str_eq/str_cmp. `call_host` records the helper
            // relocation so the chunk stays on-disk cacheable.
            let Some(helper_id) = stryke_str_helper_id(ext_id) else {
                return false;
            };
            let (Some(b), Some(a)) = (cx.pop_i64(), cx.pop_i64()) else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a, b]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if is_stryke_str_int_str_ext(ext_id) {
            // Binary string+integer→string ops (substr 2-arg, `x`-repeat): operand `a`
            // is a string handle, `b` a plain integer (marshaled unboxed by the caller).
            // Same 2-arg `call_host` path as the compares; the result is an owned string
            // handle the caller reconstructs via `from_raw_bits`.
            let Some(helper_id) = stryke_str_int_str_helper_id(ext_id) else {
                return false;
            };
            let (Some(b), Some(a)) = (cx.pop_i64(), cx.pop_i64()) else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a, b]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if is_stryke_str_int2_str_ext(ext_id) {
            // Ternary string+int+int→string ops (substr 3-arg): operand `a` is a string
            // handle, `b`/`c` plain integers. 3-arg `call_host`; owned-string result.
            let Some(helper_id) = stryke_str_int2_str_helper_id(ext_id) else {
                return false;
            };
            let (Some(c), Some(b), Some(a)) = (cx.pop_i64(), cx.pop_i64(), cx.pop_i64()) else {
                return false;
            };
            let Some(result) = cx.call_host(helper_id, &[a, b, c]) else {
                return false;
            };
            cx.push_i64(result);
            return true;
        }
        if ext_id != ext_ops::STK_MOD_FLOOR {
            return false;
        }
        // floored(a, b): truncated remainder, adjusted by +b when non-zero and
        // its sign differs from the divisor; b ∈ {0, -1} guarded to dodge traps.
        let (Some(b), Some(a)) = (cx.pop_i64(), cx.pop_i64()) else {
            return false;
        };
        let zero = cx.iconst(0);
        let one = cx.iconst(1);
        let neg1 = cx.iconst(-1);
        let is_zero = cx.icmp_eq(b, zero);
        let is_neg1 = cx.icmp_eq(b, neg1);
        let special = cx.bor(is_zero, is_neg1);
        let safe_b = cx.select(special, one, b);
        let t = cx.srem(a, safe_b);
        let xor = cx.bxor(t, safe_b);
        let signs_differ = cx.icmp_slt(xor, zero);
        let t_nonzero = cx.icmp_ne(t, zero);
        let need = cx.band(signs_differ, t_nonzero);
        let adj = cx.select(need, safe_b, zero);
        let floored = cx.iadd(t, adj);
        let result = cx.select(special, zero, floored);
        cx.push_i64(result);
        true
    }
}

/// Register strykelang's JIT extension (idempotent, process-wide) so the block
/// JIT can compile `Op::Extended(STK_MOD_FLOOR, …)`. Must run before the first
/// modulo segment is judged for eligibility.
pub(crate) fn register_stryke_jit_ext() {
    fusevm::jit::register_global_extension(std::sync::Arc::new(StrykeJitExt));
    // Register the string-compare host helpers (idempotent, process-global) so
    // both freshly-JITted and disk-cache-reloaded chunks can resolve them.
    for (_, name, ptr) in STRYKE_STR_HELPERS {
        // SAFETY: each helper is `extern "C" fn(i64, i64) -> i64`, matching the
        // 2-arg / integer-return signature declared to the JIT.
        unsafe {
            fusevm::jit::register_jit_helper(name, *ptr as *const u8, 2, false);
        }
    }
    // SAFETY: each binary string+integer→string helper is `extern "C" fn(i64, i64) ->
    // i64` (string handle + plain int → owned string handle), the same 2-arg ABI.
    for (_, name, ptr) in STRYKE_STR_INT_STR_HELPERS {
        unsafe {
            fusevm::jit::register_jit_helper(name, *ptr as *const u8, 2, false);
        }
    }
    // SAFETY: each ternary string+int+int→string helper is `extern "C" fn(i64, i64,
    // i64) -> i64` (string handle + two plain ints → owned string handle), 3-arg ABI.
    for (_, name, ptr) in STRYKE_STR_INT2_STR_HELPERS {
        unsafe {
            fusevm::jit::register_jit_helper(name, *ptr as *const u8, 3, false);
        }
    }
    // SAFETY: each unary helper is `extern "C" fn(i64) -> i64`, matching the
    // 1-arg / integer-return signature declared here. The string→string helpers
    // return an `i64` handle (raw bits of an owned string) under the same ABI.
    for (_, name, ptr) in STRYKE_STR_UNARY_HELPERS
        .iter()
        .chain(STRYKE_STR_UNARY_STR_HELPERS.iter())
        .chain(STRYKE_INT_STR_HELPERS.iter())
    {
        unsafe {
            fusevm::jit::register_jit_helper(name, *ptr as *const u8, 1, false);
        }
    }
    // SAFETY: `stryke_h_val_load_const` is `extern "C" fn(i64) -> i64` (constant
    // index → owned handle bits), same 1-arg integer-return signature.
    unsafe {
        fusevm::jit::register_jit_helper(
            STRYKE_VAL_LOAD_CONST_NAME,
            stryke_h_val_load_const as *const u8,
            1,
            false,
        );
    }
}

/// fusevm interpreter handler for strykelang Extended ops, registered on the
/// fallback `fusevm::VM` so the cold (pre-warmup) path computes the same result
/// the JIT does.
fn stryke_ext_handler(vm: &mut fusevm::VM, id: u16, _arg: u8) {
    if id == ext_ops::STK_MOD_FLOOR {
        let b = vm.pop().to_int();
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(floored_mod(a, b)));
    } else if is_stryke_str_unary_ext(id) {
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(stryke_str_unary_op(id, a)));
    } else if is_stryke_val_load_const_ext(id) {
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(stryke_val_load_const_op(a)));
    } else if is_stryke_val_unary_int_ext(id) {
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(stryke_val_unary_int_op(id, a)));
    } else if is_stryke_val_unary_str_ext(id) {
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(stryke_val_unary_str_op(id, a)));
    } else if is_stryke_str_int_str_ext(id) {
        let b = vm.pop().to_int();
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(stryke_str_int_str_op(id, a, b)));
    } else if is_stryke_str_int2_str_ext(id) {
        let c = vm.pop().to_int();
        let b = vm.pop().to_int();
        let a = vm.pop().to_int();
        vm.push(fusevm::Value::Int(stryke_str_int2_str_op(id, a, b, c)));
    } else if is_stryke_str_ext(id) {
        let b = vm.pop().to_int();
        let a = vm.pop().to_int();
        let r = if id == ext_ops::STK_STR_CONCAT {
            stryke_str_concat_op(a, b)
        } else if id == ext_ops::STK_STR_CRYPT {
            stryke_str_crypt_op(a, b)
        } else if is_stryke_str_index_ext(id) {
            stryke_str_index_op(id, a, b)
        } else {
            stryke_str_compare_op(id, a, b)
        };
        vm.push(fusevm::Value::Int(r));
    }
}


/// True when every op in `seg` is in the strict 1:1 universal-integer/slot subset
/// that translates to exactly one `fusevm::Op`, and every jump target stays inside
/// the segment `[seg_start, seg_start + seg.len()]`.
///
/// Restricting to a 1:1 mapping keeps jump remapping a fixed affine shift (the
/// preamble length) with no instruction-index rewriting. Anything outside the set
/// (floats, constants, name/arg reads, declarations, calls, arrays, hashes, fused
/// opcodes, keep-variant jumps, void-context slot mutations) makes the segment
/// ineligible and the caller falls back to strykelang's own JIT/interpreter.
///
/// `Op::Mod` is lowered to `fusevm::Op::Extended(STK_MOD_FLOOR, 0)` rather than
/// fusevm's universal `Op::Mod`, because strykelang/Perl `%` is *floored* (result
/// takes the sign of the divisor, e.g. `-7 % 3 == 2`) whereas fusevm's `Mod` is
/// truncated i64 `%` (`-7 % 3 == -1`). The extension ([`StrykeJitExt`]) JITs the
/// floored form, and [`stryke_ext_handler`] mirrors it on the interpreter
/// fallback. `Op::Div`/`Op::Pow` stay eligible but always yield a float, so
/// [`segment_result_is_integer`] routes any segment containing them through
/// fusevm's interpreter rather than the integer block JIT.
pub(crate) fn segment_is_fusevm_eligible(seg: &[Op], seg_start: usize) -> bool {
    if seg.is_empty() {
        return false;
    }
    let seg_end = seg_start + seg.len();
    for op in seg {
        match op {
            Op::LoadInt(_)
            | Op::Pop
            | Op::Dup
            | Op::Dup2
            | Op::Swap
            | Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::Pow
            | Op::Negate
            | Op::NumEq
            | Op::NumNe
            | Op::NumLt
            | Op::NumGt
            | Op::NumLe
            | Op::NumGe
            | Op::Spaceship
            | Op::LogNot
            | Op::Inc
            | Op::Dec
            | Op::BitAnd
            | Op::BitOr
            | Op::BitXor
            | Op::BitNot
            | Op::Shl
            | Op::Shr
            | Op::GetScalarSlot(_)
            | Op::SetScalarSlot(_)
            | Op::SetScalarSlotKeep(_)
            | Op::DeclareScalarSlot(_, _)
            | Op::AddAssignSlotSlot(_, _)
            | Op::SubAssignSlotSlot(_, _)
            | Op::MulAssignSlotSlot(_, _)
            | Op::AddAssignSlotSlotVoid(_, _)
            | Op::PreIncSlot(_)
            | Op::PreDecSlot(_)
            | Op::PostIncSlot(_)
            | Op::PostDecSlot(_)
            | Op::PreIncSlotVoid(_)
            | Op::AccumSumLoop(_, _, _) => continue,
            Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            // Fused loop ops carry an in-segment jump target (top check / backedge).
            Op::SlotLtIntJumpIfFalse(_, _, t) | Op::SlotIncLtIntJumpBack(_, _, t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            // Always-float math built-ins (e.g. `sqrt`/`sin`/`cos`/`exp`/`atan2`)
            // lowered to native fusevm float ops; eligible like any other op.
            _ if is_float_builtin(op) => continue,
            // strykelang `int(x)`/`ceil(x)`/`floor(x)` -> integer-producing fusevm
            // ops (Op::TruncInt alone for int; [Op::CeilFloat, Op::TruncInt] for
            // ceil; [Op::FloorFloat, Op::TruncInt] for floor).
            _ if is_int_returning_float_builtin(op) => continue,
            _ => return false,
        }
    }
    segment_block_stack_is_consistent(seg, seg_start)
}

/// Lightweight operand kind for the linear float-segment type propagation. fusevm
/// promotes mixed int/float arithmetic to float, so the bridge must know each
/// operand's kind to (a) refuse storing a float into an integer-marshaled slot and
/// (b) know whether the segment's result is an integer/bool (the only kind the
/// block JIT can return today — see [`segment_is_fusevm_float_eligible`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NumTy {
    Int,
    Float,
}

/// Apply a single straight-line (non-control-flow) op to the abstract operand-type
/// stack used by [`segment_is_fusevm_float_eligible`], returning `false` if the op is
/// not float-safe or underflows. Control-flow ops are handled by the caller.
fn float_apply_op(op: &Op, stack: &mut Vec<NumTy>) -> bool {
    use Op::*;
    match op {
        LoadInt(_) => stack.push(NumTy::Int),
        LoadFloat(_) => stack.push(NumTy::Float),
        // Runtime slots are marshaled as `i64` integers (a float arg bails to the
        // interpreter before we get here), so a slot read is always an integer.
        GetScalarSlot(_) => stack.push(NumTy::Int),
        Pop => {
            if stack.pop().is_none() {
                return false;
            }
        }
        Dup => match stack.last() {
            Some(&t) => stack.push(t),
            None => return false,
        },
        Dup2 => {
            let n = stack.len();
            if n < 2 {
                return false;
            }
            let (a, b) = (stack[n - 2], stack[n - 1]);
            stack.push(a);
            stack.push(b);
        }
        Swap => {
            let n = stack.len();
            if n < 2 {
                return false;
            }
            stack.swap(n - 1, n - 2);
        }
        Add | Sub | Mul => {
            let (b, a) = match (stack.pop(), stack.pop()) {
                (Some(b), Some(a)) => (b, a),
                _ => return false,
            };
            stack.push(if a == NumTy::Float || b == NumTy::Float {
                NumTy::Float
            } else {
                NumTy::Int
            });
        }
        // strykelang `/` is *always* float division (even `4 / 2 == 2.0`), and it
        // lowers to fusevm `Op::AwkDivJit`, which divides in floating point in both
        // the block JIT (`fdiv` over operands coerced to f64) and the interpreter,
        // and persists to the native disk cache. The result is therefore always a
        // float, independent of operand kinds. A zero divisor is handled out of
        // band (the JIT traps and the caller declines so strykelang's own
        // "Illegal division by zero" is raised), so it does not affect typing here.
        Div => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Float);
        }
        // strykelang `**` is *always* float exponentiation (even `2 ** 10 ==
        // 1024.0`), and it lowers to fusevm `Op::PowFloat`, which raises to a
        // power in floating point (operands coerced to f64 via the `pow_f64`
        // host helper) in both the block JIT and the interpreter, and persists
        // to the native disk cache. The result is therefore always a float,
        // independent of operand kinds.
        Pow => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Float);
        }
        // Unary negate preserves the operand's kind.
        Negate => {
            if stack.last().is_none() {
                return false;
            }
        }
        NumEq | NumNe | NumLt | NumGt | NumLe | NumGe | Spaceship => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Int);
        }
        LogNot => {
            if stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Int);
        }
        // Storing into an `i64`-marshaled slot: the stored value must be an integer,
        // else it would be silently truncated on the next read.
        SetScalarSlot(_) | DeclareScalarSlot(_, _) => {
            if stack.pop() != Some(NumTy::Int) {
                return false;
            }
        }
        SetScalarSlotKeep(_) => {
            if stack.last() != Some(&NumTy::Int) {
                return false;
            }
        }
        // Integer-only slot superinstructions: they neither consume nor produce a
        // float operand (slots are integers).
        AddAssignSlotSlotVoid(_, _) | PreIncSlotVoid(_) | AccumSumLoop(_, _, _) => {}
        AddAssignSlotSlot(_, _) | SubAssignSlotSlot(_, _) | MulAssignSlotSlot(_, _)
        | PreIncSlot(_) | PreDecSlot(_) | PostIncSlot(_) | PostDecSlot(_) => {
            stack.push(NumTy::Int)
        }
        // Always-float unary built-ins (`sqrt`/`sin`/`cos`/`exp`): consume one
        // operand of any kind and produce a float.
        _ if is_float_unary_builtin(op) => {
            if stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Float);
        }
        // Always-float binary built-ins (`atan2`): consume two operands and
        // produce a float.
        _ if is_float_binary_builtin(op) => {
            if stack.pop().is_none() || stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Float);
        }
        // `int(x)`/`ceil(x)`/`floor(x)`: consume one operand of either kind and
        // produce an integer (fusevm sequence ends in `Op::TruncInt`). This is
        // what lets a float-bearing segment (e.g. `int($x / 2)`, `ceil($x - 0.5)`)
        // yield a block-JIT-returnable integer.
        _ if is_int_returning_float_builtin(op) => {
            if stack.pop().is_none() {
                return false;
            }
            stack.push(NumTy::Int);
        }
        // Everything else — `Mod`, bit/shift ops, `Inc`/`Dec`, fused-loop
        // ops, string ops, plain-slot reads — is not float-safe here.
        _ => return false,
    }
    true
}

/// Whether `seg` is eligible for the *float-operand* block-JIT path: it contains at
/// least one `LoadFloat` (otherwise the plain integer path in
/// [`segment_is_fusevm_eligible`] already covers it), every op's fusevm lowering is
/// semantically identical to strykelang for both integer **and** float operands,
/// **and** the segment's single result is an integer/bool.
///
/// The integer-result restriction is the key correctness constraint: fusevm's block
/// JIT returns its top-of-stack as an `i64`, truncating a float result to an integer
/// (`scalar_store_i64`). So only segments whose result is an integer/bool — e.g. a
/// float comparison `$x < 0.5`, `$a >= 1.5`, a `!`/logical test over them, or a
/// ternary `$x < 0.5 ? 1 : 0` whose branches both yield integers — may take this
/// path; the comparison itself runs in floating point (fusevm promotes the operands)
/// and yields the exact 0/1 the interpreter would.
///
/// This propagates the abstract operand *type* stack along every control-flow edge
/// (`Jump`/`JumpIfTrue`/`JumpIfFalse`, targets resolved relative to `seg_start`) and
/// bails on any disagreement at a control-flow merge — both the stack *depth* and the
/// per-slot Int/Float *kind* must match across all incoming edges — so the type the
/// block JIT actually computes is exactly the one analysed here. (fusevm independently
/// bails to the interpreter on a block-param type mismatch, so a missed disagreement
/// degrades to the interpreter rather than miscompiling.)
///
/// Admitted: `LoadInt`/`LoadFloat`, stack shuffles, `+ - *`, unary negate, the six
/// numeric comparisons, `Spaceship`, `LogNot`, the integer-only slot ops (slots are
/// marshaled as `i64`), and `Jump`/`JumpIf{True,False}`. Rejected: `Div`/`Pow`
/// (strykelang's `/` and `**` are always float, but fusevm's `Op::Div` is integer
/// division on two ints), `Mod`, every bit/shift op and `Inc`/`Dec` (unsafe on a
/// float operand), the fused-loop ops, and storing a float into a slot.
pub(crate) fn segment_is_fusevm_float_eligible(seg: &[Op], seg_start: usize) -> bool {
    segment_fusevm_float_result_kind(seg, seg_start).is_some()
}

/// Core of [`segment_is_fusevm_float_eligible`]: returns the segment's single
/// numeric result kind (`Int` or `Float`) when the float-operand block-JIT path
/// applies, or `None` if the segment is ineligible (not exactly one result, an
/// unmodelable/float-unsafe op, a stack underflow, or a control-flow merge
/// disagreement).
///
/// A `Float` result is now JIT-eligible: fusevm's block tier returns it as the
/// raw `f64` bit pattern (via [`fusevm::JitCompiler::try_run_block_typed_kinded`])
/// rather than truncating it, so the caller reconstructs the exact float. Float
/// stores into `i64`-marshaled slots remain rejected by `float_apply_op`.
pub(crate) fn segment_fusevm_float_result_kind(seg: &[Op], seg_start: usize) -> Option<NumTy> {
    use Op::*;
    // The float-operand path applies to a segment that either carries an explicit
    // float literal (`LoadFloat`), performs strykelang's always-float division
    // (`Div`, lowered to fusevm `Op::AwkDivJit`), always-float exponentiation
    // (`Pow`, lowered to fusevm `Op::PowFloat`), or an always-float unary built-in
    // (`sqrt`, lowered to fusevm `Op::SqrtFloat`). A segment with none of these is
    // pure integer and is already covered by `segment_is_fusevm_eligible`.
    if !seg
        .iter()
        .any(|o| matches!(o, LoadFloat(_) | Div | Pow) || is_float_builtin(o))
    {
        return None;
    }
    let n = seg.len();
    // Resolve an absolute jump target to a segment-relative index in `0..=n`
    // (`n` is the implicit end / return point).
    let rel = |t: usize| -> Option<usize> { t.checked_sub(seg_start).filter(|r| *r <= n) };

    // Abstract operand-type stack at the *entry* of each ip (0..=n); `None` = not yet
    // reached. A merge requires the incoming stack to equal the recorded one.
    let mut state: Vec<Option<Vec<NumTy>>> = vec![None; n + 1];
    state[0] = Some(Vec::new());
    let mut work: Vec<usize> = vec![0];

    // Merge an incoming type-stack into `target`, queueing it on first arrival and
    // rejecting any later mismatch.
    let merge = |state: &mut Vec<Option<Vec<NumTy>>>,
                     work: &mut Vec<usize>,
                     target: usize,
                     incoming: &[NumTy]|
     -> bool {
        match &state[target] {
            None => {
                state[target] = Some(incoming.to_vec());
                work.push(target);
                true
            }
            Some(existing) => existing.as_slice() == incoming,
        }
    };

    while let Some(ip) = work.pop() {
        if ip >= n {
            continue;
        }
        let mut stk = state[ip].clone().expect("queued ip has known state");
        match &seg[ip] {
            Jump(t) => {
                let Some(tr) = rel(*t) else { return None };
                if !merge(&mut state, &mut work, tr, &stk) {
                    return None;
                }
            }
            JumpIfTrue(t) | JumpIfFalse(t) => {
                // Both branches pop the condition; the kind is irrelevant.
                if stk.pop().is_none() {
                    return None;
                }
                let Some(tr) = rel(*t) else { return None };
                if !merge(&mut state, &mut work, tr, &stk)
                    || !merge(&mut state, &mut work, ip + 1, &stk)
                {
                    return None;
                }
            }
            other => {
                if !float_apply_op(other, &mut stk) {
                    return None;
                }
                if !merge(&mut state, &mut work, ip + 1, &stk) {
                    return None;
                }
            }
        }
    }

    // The result is the operand-stack state at the implicit return point: exactly
    // one value, of either kind (the block JIT can now return both).
    match &state[n] {
        Some(s) if s.len() == 1 => Some(s[0]),
        _ => None,
    }
}

/// Per-op operand-stack depth change for the ops that may appear in an eligible
/// integer/string segment. `None` for any op we don't model (forces the caller to
/// treat the segment as ineligible). Mirrors the lowering in [`translate_op_into`].
fn op_stack_delta(op: &Op) -> Option<i32> {
    use Op::*;
    Some(match op {
        // `LoadConst(idx)` translates to `[LoadInt(idx), Extended(STK_VAL_LOAD_CONST)]`
        // in [`translate_op_into`] — net stack delta +1, same as a plain `LoadInt`.
        // Modeling it here lets compare/index/binary-str detectors accept `LoadConst`
        // operand positions without bailing on stack-consistency.
        LoadInt(_) | LoadConst(_) | Dup | GetScalarSlot(_) | GetScalarPlain(_) => 1,
        Dup2 => 2,
        Pop | SetScalarSlot(_) | DeclareScalarSlot(_, _) => -1,
        Swap | SetScalarSlotKeep(_) | Negate | BitNot | LogNot | Inc | Dec
        | PreIncSlotVoid(_) | AddAssignSlotSlotVoid(_, _) | AccumSumLoop(_, _, _)
        | SlotLtIntJumpIfFalse(_, _, _) | SlotIncLtIntJumpBack(_, _, _) | Jump(_) => 0,
        Add | Sub | Mul | Div | Mod | Pow | BitAnd | BitOr | BitXor | Shl | Shr
        | NumEq | NumNe | NumLt | NumGt | NumLe | NumGe | Spaceship
        | StrEq | StrNe | StrLt | StrGt | StrLe | StrGe | StrCmp | Concat
        | JumpIfTrue(_) | JumpIfFalse(_) => -1,
        AddAssignSlotSlot(_, _) | SubAssignSlotSlot(_, _) | MulAssignSlotSlot(_, _)
        | PreIncSlot(_) | PreDecSlot(_) | PostIncSlot(_) | PostDecSlot(_) => 1,
        // `int(x)`/`ceil(x)`/`floor(x)` pop one and push one (net 0); see
        // `is_int_returning_float_builtin`.
        _ if is_int_returning_float_builtin(op) => 0,
        _ => return None,
    })
}

/// fusevm's block JIT carries operand-stack values that are live across a basic-block
/// boundary as Cranelift block parameters (typed by the value), so a ternary `?:` /
/// `if`-expression result that merges at a join point — or is carried by a
/// value-bearing jump to the implicit return point — survives the merge and is JIT
/// (and disk-cache) eligible. The one requirement is that the operand-stack **depth**
/// at every control-flow merge is consistent across all incoming edges (fusevm fixes
/// each block's parameter arity at its first predecessor; a divergent arity would make
/// it bail to the interpreter). Counted/accumulator loops trivially satisfy this (their
/// cross-iteration state lives in slots and every boundary is reached at depth 0).
///
/// This propagates operand-stack depth along every control-flow edge and returns
/// `false` only on a genuinely malformed segment: an inconsistent merge depth, an
/// unmodelable op, or a stack underflow. (Type consistency across a merge is enforced
/// by fusevm itself, which safely bails to the interpreter on a mismatch rather than
/// miscompiling, so it need not be re-checked here.)
fn segment_block_stack_is_consistent(seg: &[Op], seg_start: usize) -> bool {
    let n = seg.len();
    // Resolve an absolute jump target to a segment-relative index in `0..=n`
    // (`n` is the implicit end / return point).
    let rel = |t: usize| -> Option<usize> { t.checked_sub(seg_start).filter(|r| *r <= n) };

    let mut depth_in: Vec<Option<i32>> = vec![None; n + 1];
    depth_in[0] = Some(0);
    let mut work: Vec<usize> = vec![0];
    while let Some(ip) = work.pop() {
        if ip >= n {
            continue;
        }
        let d = depth_in[ip].expect("queued ip has known depth");
        let delta = match op_stack_delta(&seg[ip]) {
            Some(v) => v,
            None => return false,
        };
        let after = d + delta;
        if after < 0 {
            return false;
        }
        // Successors: (target_index, reached_via_jump).
        let mut succs: Vec<(usize, bool)> = Vec::new();
        match &seg[ip] {
            Op::Jump(t) => match rel(*t) {
                Some(r) => succs.push((r, true)),
                None => return false,
            },
            Op::JumpIfTrue(t)
            | Op::JumpIfFalse(t)
            | Op::SlotLtIntJumpIfFalse(_, _, t)
            | Op::SlotIncLtIntJumpBack(_, _, t) => {
                match rel(*t) {
                    Some(r) => succs.push((r, true)),
                    None => return false,
                }
                succs.push((ip + 1, false));
            }
            _ => succs.push((ip + 1, false)),
        }
        for (s, _via_jump) in succs {
            // Every merge (including the implicit end point `n`) must be reached at a
            // single consistent operand-stack depth across all incoming edges.
            match depth_in[s] {
                Some(prev) => {
                    if prev != after {
                        return false;
                    }
                }
                None => {
                    depth_in[s] = Some(after);
                    work.push(s);
                }
            }
        }
    }
    true
}

/// True when `seg` is a pure string-comparison body: only slot reads feeding the
/// `eq ne lt gt le ge cmp` ops (plus boolean `LogNot` / stack shuffles), with at
/// least one such op present. These segments are marshaled with **raw NaN-boxed
/// `StrykeValue` bits** as slot handles (rather than unboxed integers), so they
/// are kept strictly disjoint from the integer subset: no arithmetic or numeric
/// op may appear, since those would misinterpret the handle bits.
pub(crate) fn segment_is_string_compare_eligible(seg: &[Op], seg_start: usize) -> bool {
    if seg.is_empty() {
        return false;
    }
    let seg_end = seg_start + seg.len();
    let mut has_str_op = false;
    for op in seg {
        match op {
            Op::StrEq | Op::StrNe | Op::StrLt | Op::StrGt | Op::StrLe | Op::StrGe
            | Op::StrCmp => has_str_op = true,
            // `LoadConst` is the literal-string case: `$x eq "literal"`,
            // `"prefix" lt $y`, chained `$a eq "y" || $a eq "n"`, etc. The
            // [`STK_VAL_LOAD_CONST`] translation pushes a borrowed handle that
            // the compare helpers consume via `sv_borrow` — identical contract
            // to seeded-slot operands.
            Op::GetScalarSlot(_) | Op::LoadConst(_) | Op::LogNot | Op::Pop | Op::Dup => {}
            Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t) => {
                if *t < seg_start || *t > seg_end {
                    return false;
                }
            }
            _ => return false,
        }
    }
    has_str_op && segment_block_stack_is_consistent(seg, seg_start)
}

/// True when `seg` is exactly a single string concatenation `$x . $y` of two slot
/// operands: `[GetScalarSlot, GetScalarSlot, Concat]`. This deliberately strict
/// shape is what makes the allocating concat safe to lower:
///
/// * Both operands are borrowed slot handles (never dropped by the JIT body), so
///   the caller's `Arc`s are untouched.
/// * The single `Concat` produces exactly one *owned* result handle, returned as
///   the segment value and reconstituted into one owning `StrykeValue` by
///   [`run_linear_segment`]. There are no stack shuffles (`Dup`/`Pop`/`Swap`) and
///   no chained concat, so there is never an owned *intermediate* handle that
///   could leak or be double-freed.
///
/// Like the compare segments, operands are marshaled as raw NaN-boxed bits and the
/// chunk is built unseeded, so its `op_hash` (the disk-cache key) is independent of
/// the per-run operand pointers.
pub(crate) fn segment_is_string_concat_eligible(seg: &[Op], _seg_start: usize) -> bool {
    // `[GetScalarSlot, GetScalarSlot, Concat]` is the original shape: `$a . $b`
    // with both operands as scope-local scalars. `LoadConst` in either position
    // is the new `[…, "literal", Concat]` / `[ "literal", …, Concat]` shape, with
    // the literal resolved at runtime via [`ext_ops::STK_VAL_LOAD_CONST`] off the
    // thread-local constants pool (see [`LoadConstCtxGuard`]). LoadConst-on-both
    // is unreachable in compiled code (the compiler constant-folds it), but
    // matching it costs nothing and avoids a brittle assertion if the folder
    // ever changes.
    matches!(
        seg,
        [Op::GetScalarSlot(_), Op::GetScalarSlot(_), Op::Concat]
            | [Op::GetScalarSlot(_), Op::LoadConst(_), Op::Concat]
            | [Op::LoadConst(_), Op::GetScalarSlot(_), Op::Concat]
            | [Op::LoadConst(_), Op::LoadConst(_), Op::Concat]
    )
}

/// Maps a binary string→int builtin call (2-arg `index`/`rindex`) to its `STK_STR_*`
/// Extended id, if any. Two string-handle operands, `i64` result — same raw-bits
/// marshaling and integer-result handling as the comparison ops.
#[inline]
fn binary_str_int_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    let Op::CallBuiltin(id, 2) = op else {
        return None;
    };
    let id = *id;
    if id == BuiltinId::Index as u16 {
        Some(ext_ops::STK_STR_INDEX)
    } else if id == BuiltinId::Rindex as u16 {
        Some(ext_ops::STK_STR_RINDEX)
    } else {
        None
    }
}

/// Maps a binary string→**string** builtin call (currently 2-arg `crypt`) to its
/// `STK_STR_*` Extended id. Two string-handle operands, owned-string-handle
/// result — same `(i64, i64) -> i64` ABI as `STK_STR_CONCAT`. The pipeline
/// must reconstruct the result via `from_raw_bits` (not box it as an integer).
#[inline]
fn binary_str_str_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    let Op::CallBuiltin(id, 2) = op else {
        return None;
    };
    let id = *id;
    if id == BuiltinId::Crypt as u16 {
        Some(ext_ops::STK_STR_CRYPT)
    } else {
        None
    }
}

/// True when `seg` is a single binary string→string builtin (currently 2-arg
/// `crypt`) of two operand-producing ops: each may be either a `GetScalarSlot`
/// (`crypt($a, $salt)`) or a `LoadConst` (`crypt($a, "salt")` /
/// `crypt("seed", $key)`), in any combination. Mirrors
/// [`segment_is_string_binary_int_eligible`] — same `(handle, handle) -> handle`
/// ABI; result is an owned string handle reconstructed via `from_raw_bits`,
/// like [`stryke_str_concat_op`].
pub(crate) fn segment_is_string_binary_str_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(
        seg,
        [a, b, u]
            if is_str_operand(a) && is_str_operand(b) && binary_str_str_ext_op(u).is_some()
    )
}

/// True when `seg` is a single binary string→int builtin (`index`/`rindex`,
/// 2-arg) of two operand-producing ops: each may be either a `GetScalarSlot` or
/// a `LoadConst` (so `index($s, "needle")` and `index("haystack", $n)` both
/// lower). Operands marshal as raw NaN-boxed `StrykeValue` bits — for slots,
/// the caller's `is_string_like` seeder gate enforces plain-string; for
/// LoadConst, the [`STK_VAL_LOAD_CONST`] runtime resolution borrows the
/// constants-pool handle (also string-typed in compiled code, since
/// `index`/`rindex` reach here only with string arguments). Result is a plain
/// `i64`, so the chunk stays on the integer block JIT + on-disk cache.
pub(crate) fn segment_is_string_binary_int_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(
        seg,
        [a, b, u]
            if is_str_operand(a) && is_str_operand(b) && binary_str_int_ext_op(u).is_some()
    )
}

/// True for an op that produces a single string handle on the stack — either a
/// `GetScalarSlot` (slot-seeded, gated by `is_string_like`) or a `LoadConst`
/// (resolved at runtime via [`ext_ops::STK_VAL_LOAD_CONST`] off the
/// thread-local constants pool). Shared by every detector that accepts
/// "string-producing operand at this position": compare, concat, index/rindex,
/// crypt.
#[inline]
fn is_str_operand(op: &Op) -> bool {
    matches!(op, Op::GetScalarSlot(_) | Op::LoadConst(_))
}

/// Maps a unary string→int builtin call to its `STK_STR_*` Extended id, if any:
/// `length`/`ord`/`hex`/`oct`. All take one operand and yield an `i64`, so they
/// share the same segment shape, raw-bits marshaling and integer-result handling.
#[inline]
fn unary_str_int_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    let Op::CallBuiltin(id, 1) = op else {
        return None;
    };
    let id = *id;
    if id == BuiltinId::Length as u16 {
        Some(ext_ops::STK_STR_LEN)
    } else if id == BuiltinId::Ord as u16 {
        Some(ext_ops::STK_STR_ORD)
    } else if id == BuiltinId::Hex as u16 {
        Some(ext_ops::STK_STR_HEX)
    } else if id == BuiltinId::Oct as u16 {
        Some(ext_ops::STK_STR_OCT)
    } else {
        None
    }
}

/// Maps a unary string→**string** builtin call to its `STK_STR_*` Extended id, if
/// any: `uc`/`lc`/`ucfirst`/`lcfirst`. Same single-operand segment shape and raw-bits
/// marshaling as the string→int family, but the result is an owned string handle
/// (reconstructed via `from_raw_bits`, like concatenation) rather than an integer.
#[inline]
fn unary_str_str_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    let Op::CallBuiltin(id, 1) = op else {
        return None;
    };
    let id = *id;
    if id == BuiltinId::Uc as u16 {
        Some(ext_ops::STK_STR_UC)
    } else if id == BuiltinId::Lc as u16 {
        Some(ext_ops::STK_STR_LC)
    } else if id == BuiltinId::Ucfirst as u16 {
        Some(ext_ops::STK_STR_UCFIRST)
    } else if id == BuiltinId::Lcfirst as u16 {
        Some(ext_ops::STK_STR_LCFIRST)
    } else if id == BuiltinId::Fc as u16 {
        Some(ext_ops::STK_STR_FC)
    } else if id == BuiltinId::Quotemeta as u16 {
        Some(ext_ops::STK_STR_QUOTEMETA)
    } else if id == BuiltinId::Reverse as u16 {
        Some(ext_ops::STK_STR_REVERSE)
    } else {
        None
    }
}

/// Maps any unary string builtin call (`length`/`ord`/`hex`/`oct` →int, or
/// `uc`/`lc`/`ucfirst`/`lcfirst` →string handle) to its `STK_STR_*` Extended id.
#[inline]
fn unary_str_ext_op(op: &Op) -> Option<u16> {
    unary_str_int_ext_op(op).or_else(|| unary_str_str_ext_op(op))
}

/// Maps the int→string builtin `chr` to its `STK_STR_CHR` Extended id. The operand
/// is an integer (marshaled unboxed) and the result is an owned string handle.
#[inline]
fn int_str_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    match op {
        Op::CallBuiltin(id, 1) if *id == BuiltinId::Chr as u16 => Some(ext_ops::STK_STR_CHR),
        _ => None,
    }
}

/// True when `op` is any unary string builtin lowered to a `STK_STR_*` unary ext op.
#[inline]
fn is_unary_str_int_builtin(op: &Op) -> bool {
    unary_str_ext_op(op).is_some()
}

/// True when `seg` is exactly a single unary string→int builtin of one slot operand:
/// `[GetScalarSlot, CallBuiltin(length|ord|hex|oct, 1)]`. Like the compare/concat
/// segments the operand is marshaled as raw NaN-boxed `StrykeValue` bits (and routed
/// only when it is a plain string — see the caller's `is_string_like` gate), and the
/// result is a plain `i64`, so the chunk stays on the integer block JIT + disk cache.
pub(crate) fn segment_is_string_unary_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(seg, [Op::GetScalarSlot(_), u] if is_unary_str_int_builtin(u))
}

/// True for an integer arithmetic / compare op that takes two integer operands
/// (e.g. the int produced by a `unary_str_int` builtin and a `LoadInt` literal)
/// and produces an integer result. Used by
/// [`segment_is_string_unary_int_combined_eligible`] to allow patterns like
/// `length($s) > 5`, `length($s) == 0`, `length($s) - 1` to lower.
#[inline]
fn is_int_binary_op(op: &Op) -> bool {
    matches!(
        op,
        Op::Add
            | Op::Sub
            | Op::Mul
            | Op::Div
            | Op::Mod
            | Op::BitAnd
            | Op::BitOr
            | Op::BitXor
            | Op::Shl
            | Op::Shr
            | Op::NumEq
            | Op::NumNe
            | Op::NumLt
            | Op::NumGt
            | Op::NumLe
            | Op::NumGe
            | Op::Spaceship
    )
}

/// True when `seg` is `[GetScalarSlot, CallBuiltin(length|ord|hex|oct, 1),
/// LoadInt(_), int_binop]` — a unary string→int builtin's result combined with
/// an integer literal via a binary integer op. Covers the universal
/// `length($s) > N` / `length($s) == 0` / `length($s) - 1` patterns and
/// `ord($c) >= 0x80` Unicode-range checks.
///
/// Slot marshals as a NaN-boxed string handle (the only operand is the
/// `GetScalarSlot` consumed immediately by the unary str builtin); the
/// `LoadInt` and result are plain `i64`s. Goes through the same str-family
/// dispatch as [`segment_is_string_unary_eligible`] in [`run_linear_segment`].
pub(crate) fn segment_is_string_unary_int_combined_eligible(
    seg: &[Op],
    _seg_start: usize,
) -> bool {
    matches!(
        seg,
        [Op::GetScalarSlot(_), u, Op::LoadInt(_), b]
            if is_unary_str_int_builtin(u) && is_int_binary_op(b)
    )
}

/// True when `seg` is `[GetScalarSlot, unary_str_int, GetScalarSlot, unary_str_int,
/// int_binop]` — two unary string→int builtins on two (possibly identical)
/// slots combined via a binary integer op. Covers the universal
/// `length($a) > length($b)` / `length($a) - length($b)` /
/// `ord($a) == ord($b)` patterns.
///
/// Both slots marshal as NaN-boxed string handles (each consumed immediately
/// by its own unary str builtin); the result is a plain `i64`. Same str-family
/// `unary_ok` dispatch as the single-slot + int-literal variant — the seeder
/// treats every slot in a str_ok segment as a string handle, which is what
/// both `GetScalarSlot`s need here.
pub(crate) fn segment_is_string_unary_binop_eligible(
    seg: &[Op],
    _seg_start: usize,
) -> bool {
    matches!(
        seg,
        [Op::GetScalarSlot(_), u1, Op::GetScalarSlot(_), u2, b]
            if is_unary_str_int_builtin(u1)
                && is_unary_str_int_builtin(u2)
                && is_int_binary_op(b)
    )
}

/// True when `seg` is a unary string→**string** builtin segment
/// (`[GetScalarSlot, CallBuiltin(uc|lc|ucfirst|lcfirst, 1)]`), whose JIT result is
/// the raw bits of an owned string handle (reconstructed via `from_raw_bits`) rather
/// than a plain integer. A strict subset of [`segment_is_string_unary_eligible`].
pub(crate) fn segment_is_string_unary_str_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(seg, [Op::GetScalarSlot(_), u] if unary_str_str_ext_op(u).is_some())
}

/// True when `seg` is an int→string builtin segment
/// (`[GetScalarSlot, CallBuiltin(chr, 1)]`): the operand is an **integer** (marshaled
/// unboxed, like the integer fast path) and the result is an owned string handle
/// (reconstructed via `from_raw_bits`, like concatenation). Its own eligibility class
/// because the operand marshaling differs from the string-operand unary family.
pub(crate) fn segment_is_int_to_string_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(seg, [Op::GetScalarSlot(_), u] if int_str_ext_op(u).is_some())
}

/// Maps a binary string+INTEGER→string op to its `STK_STR_*` Extended id: the 2-arg
/// `substr($s,$off)` builtin and the `$s x $n` repeat operator (`Op::StringRepeat`).
/// The first operand is a string handle, the second a plain integer.
#[inline]
fn str_int_str_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    match op {
        Op::StringRepeat => Some(ext_ops::STK_STR_REPEAT),
        Op::CallBuiltin(id, 2) if *id == BuiltinId::Substr as u16 => Some(ext_ops::STK_STR_SUBSTR2),
        _ => None,
    }
}

/// True when `seg` is a binary string+integer→string segment
/// (`[GetScalarSlot(s), GetScalarSlot(n), substr|x-repeat]`) whose two operands marshal
/// with DIFFERENT kinds: `s` as a NaN-boxed string handle, `n` as a plain integer. The
/// two slots must be distinct (a shared slot can't be marshaled as both a string and an
/// integer). The JIT result is an owned string handle (reconstructed via `from_raw_bits`).
pub(crate) fn segment_is_string_int_to_string_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(
        seg,
        [Op::GetScalarSlot(s), Op::GetScalarSlot(n), u]
            if s != n && str_int_str_ext_op(u).is_some()
    )
}

/// Maps a ternary string+INTEGER+INTEGER→string op to its `STK_STR_*` Extended id: the
/// 3-arg `substr($s,$off,$len)` builtin. The first operand is a string handle, the rest
/// plain integers.
#[inline]
fn str_int2_str_ext_op(op: &Op) -> Option<u16> {
    use crate::bytecode::BuiltinId;
    match op {
        Op::CallBuiltin(id, 3) if *id == BuiltinId::Substr as u16 => Some(ext_ops::STK_STR_SUBSTR3),
        _ => None,
    }
}

/// True when `seg` is a ternary string+int+int→string segment
/// (`[GetScalarSlot(s), GetScalarSlot(o), GetScalarSlot(l), substr-3arg]`): `s` marshals
/// as a string handle, `o`/`l` as plain integers. The string slot must be distinct from
/// both integer slots (it can't be reinterpreted as an integer). Owned-string result.
pub(crate) fn segment_is_string_int2_to_string_eligible(seg: &[Op], _seg_start: usize) -> bool {
    matches!(
        seg,
        [Op::GetScalarSlot(s), Op::GetScalarSlot(o), Op::GetScalarSlot(l), u]
            if s != o && s != l && str_int2_str_ext_op(u).is_some()
    )
}

/// True for any mixed string+integer→string segment (binary `substr`-2/`x`-repeat or
/// ternary `substr`-3), whose operands marshal per-slot (string handle + plain ints).
#[inline]
pub(crate) fn segment_is_string_int_mixed_eligible(seg: &[Op], seg_start: usize) -> bool {
    segment_is_string_int_to_string_eligible(seg, seg_start)
        || segment_is_string_int2_to_string_eligible(seg, seg_start)
}

/// True for a literal-string + slot-int(s) → string segment: the universal
/// `"prefix" x $n` / `substr("abc", $n)` (binary) and `substr("abc", $o, $l)`
/// (ternary) shapes. The string operand comes from [`Op::LoadConst`]
/// (resolved at runtime via [`ext_ops::STK_VAL_LOAD_CONST`] — disk-cache safe
/// by construction), the integer operands from slots (marshaled unboxed).
/// The result is an owned string handle, reconstructed via `from_raw_bits` by
/// the caller. Distinct from [`segment_is_string_int_to_string_eligible`] /
/// [`segment_is_string_int2_to_string_eligible`] because no slot holds a
/// string handle here — `vm.rs` seeds every slot in the segment purely as an
/// integer.
pub(crate) fn segment_is_literal_string_int_to_string_eligible(
    seg: &[Op],
    _seg_start: usize,
) -> bool {
    matches!(
        seg,
        // 2-arg: substr("abc", $n) / "abc" x $n
        [Op::LoadConst(_), Op::GetScalarSlot(_), u]
            if str_int_str_ext_op(u).is_some()
    ) || matches!(
        seg,
        // 3-arg: substr("abc", $off, $len). Slots must be distinct so neither
        // is reinterpreted as the other's value.
        [Op::LoadConst(_), Op::GetScalarSlot(o), Op::GetScalarSlot(l), u]
            if o != l && str_int2_str_ext_op(u).is_some()
    )
}

/// The slot supplying the *string* operand of a mixed string+integer→string segment
/// (binary or ternary). Every other `GetScalarSlot` in the segment marshals as a plain
/// integer. Returns `None` for non-mixed (or shared-slot) segments. The caller
/// (`vm.rs`) consults this to marshal each slot with its own kind.
pub(crate) fn string_handle_slot(seg: &[Op], _seg_start: usize) -> Option<u8> {
    match seg {
        [Op::GetScalarSlot(s), Op::GetScalarSlot(n), u]
            if s != n && str_int_str_ext_op(u).is_some() =>
        {
            Some(*s)
        }
        [Op::GetScalarSlot(s), Op::GetScalarSlot(o), Op::GetScalarSlot(l), u]
            if s != o && s != l && str_int2_str_ext_op(u).is_some() =>
        {
            Some(*s)
        }
        _ => None,
    }
}

/// Translate a single eligible strykelang op, appending the equivalent
/// `fusevm::Op`(s) to `body`. Most ops are 1:1; strykelang's fused slot ops are
/// decomposed into fusevm's primitive slot/arith ops (fusevm has no slot-slot
/// arith-assign other than `AddAssignSlotVoid`). Jump ops are emitted with a
/// placeholder target and recorded in `fixups` as `(body_index, src_target_idx)`
/// where `src_target_idx = absolute_target - seg_start`; [`build_chunk`] resolves
/// them after the body length per source op is known.
///
/// Returns `false` if `op` is unsupported.
fn translate_op_into(
    op: &Op,
    seg_start: usize,
    body: &mut Vec<fusevm::Op>,
    fixups: &mut Vec<(usize, usize)>,
    constants: &[StrykeValue],
) -> bool {
    use fusevm::Op as F;
    match op {
        Op::LoadInt(n) => body.push(F::LoadInt(*n)),
        Op::LoadFloat(f) => body.push(F::LoadFloat(*f)),
        // Strykelang LoadConst translates to a runtime constants lookup so the
        // chunk stays disk-cache safe: we emit `LoadInt(idx)` followed by an
        // Extended call to [`ext_ops::STK_VAL_LOAD_CONST`], whose helper reads
        // the constant from the thread-local pool published by
        // [`LoadConstCtxGuard`] in `run_linear_segment`. `op_hash` hashes the
        // index (stable across runs) and the extension id (stable via FNV-1a
        // over the helper name), never the per-process heap-pointer bits of the
        // resolved constant — so the same cached native blob is reused across
        // processes even for string constants whose `raw_bits` differ each run.
        // Eligibility detectors that allow `LoadConst` operands MUST run inside
        // a [`LoadConstCtxGuard`] scope before `try_run_block`, otherwise the
        // helper returns `UNDEF` and the segment computes the wrong value.
        Op::LoadConst(idx) => {
            let _ = constants;
            body.push(F::LoadInt(*idx as i64));
            body.push(F::Extended(ext_ops::STK_VAL_LOAD_CONST, 0));
        }
        Op::Pop => body.push(F::Pop),
        Op::Dup => body.push(F::Dup),
        Op::Dup2 => body.push(F::Dup2),
        Op::Swap => body.push(F::Swap),
        Op::Add => body.push(F::Add),
        Op::Sub => body.push(F::Sub),
        Op::Mul => body.push(F::Mul),
        Op::Div => body.push(F::AwkDivJit),
        Op::Mod => body.push(F::Extended(ext_ops::STK_MOD_FLOOR, 0)),
        // String comparisons lower to host-helper-backed Extended ops; operands
        // are raw StrykeValue bit-handles (see `segment_is_string_compare_eligible`).
        Op::StrEq => body.push(F::Extended(ext_ops::STK_STR_EQ, 0)),
        Op::StrNe => body.push(F::Extended(ext_ops::STK_STR_NE, 0)),
        Op::StrLt => body.push(F::Extended(ext_ops::STK_STR_LT, 0)),
        Op::StrGt => body.push(F::Extended(ext_ops::STK_STR_GT, 0)),
        Op::StrLe => body.push(F::Extended(ext_ops::STK_STR_LE, 0)),
        Op::StrGe => body.push(F::Extended(ext_ops::STK_STR_GE, 0)),
        Op::StrCmp => body.push(F::Extended(ext_ops::STK_STR_CMP, 0)),
        // String concatenation allocates a new owned string handle (see
        // `segment_is_string_concat_eligible` / `stryke_str_concat_op`).
        Op::Concat => body.push(F::Extended(ext_ops::STK_STR_CONCAT, 0)),
        Op::Pow => body.push(F::PowFloat),
        Op::Negate => body.push(F::Negate),
        Op::NumEq => body.push(F::NumEq),
        Op::NumNe => body.push(F::NumNe),
        Op::NumLt => body.push(F::NumLt),
        Op::NumGt => body.push(F::NumGt),
        Op::NumLe => body.push(F::NumLe),
        Op::NumGe => body.push(F::NumGe),
        Op::Spaceship => body.push(F::Spaceship),
        Op::LogNot => body.push(F::LogNot),
        Op::Inc => body.push(F::Inc),
        Op::Dec => body.push(F::Dec),
        Op::BitAnd => body.push(F::BitAnd),
        Op::BitOr => body.push(F::BitOr),
        Op::BitXor => body.push(F::BitXor),
        Op::BitNot => body.push(F::BitNot),
        Op::Shl => body.push(F::Shl),
        Op::Shr => body.push(F::Shr),
        Op::GetScalarSlot(s) => body.push(F::GetSlot(*s as u16)),
        Op::SetScalarSlot(s) => body.push(F::SetSlot(*s as u16)),
        // `my $x = EXPR`: the init value is already on the stack; storing it is
        // exactly `SetSlot`. The name-pool index is only for closure capture,
        // which never occurs in the pure-integer subset.
        Op::DeclareScalarSlot(s, _) => body.push(F::SetSlot(*s as u16)),
        // Store but leave the value on the stack: store, then reload.
        Op::SetScalarSlotKeep(s) => {
            body.push(F::SetSlot(*s as u16));
            body.push(F::GetSlot(*s as u16));
        }
        // `$d += $s` (void): fusevm has the exact fused op.
        Op::AddAssignSlotSlotVoid(d, s) => {
            body.push(F::AddAssignSlotVoid(*d as u16, *s as u16))
        }
        // `$d += $s` / `-=` / `*=` (push result): compute into the slot and
        // reload the new value onto the stack.
        Op::AddAssignSlotSlot(d, s) => emit_slot_assign(body, *d, *s, F::Add),
        Op::SubAssignSlotSlot(d, s) => emit_slot_assign(body, *d, *s, F::Sub),
        Op::MulAssignSlotSlot(d, s) => emit_slot_assign(body, *d, *s, F::Mul),
        Op::PreIncSlot(s) => body.push(F::PreIncSlot(*s as u16)),
        Op::PreDecSlot(s) => body.push(F::PreDecSlot(*s as u16)),
        Op::PostIncSlot(s) => body.push(F::PostIncSlot(*s as u16)),
        Op::PostDecSlot(s) => body.push(F::PostDecSlot(*s as u16)),
        Op::PreIncSlotVoid(s) => body.push(F::PreIncSlotVoid(*s as u16)),
        // `while $i < limit { $sum += $i; $i += 1 }` — fully fused; no jump target.
        Op::AccumSumLoop(sum, i, limit) => {
            body.push(F::AccumSumLoop(*sum as u16, *i as u16, *limit))
        }
        // Fused loop ops with an in-segment jump target (resolved in pass 2).
        Op::SlotLtIntJumpIfFalse(s, limit, t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::SlotLtIntJumpIfFalse(*s as u16, *limit, 0));
        }
        Op::SlotIncLtIntJumpBack(s, limit, t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::SlotIncLtIntJumpBack(*s as u16, *limit, 0));
        }
        Op::Jump(t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::Jump(0));
        }
        Op::JumpIfTrue(t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::JumpIfTrue(0));
        }
        Op::JumpIfFalse(t) => {
            fixups.push((body.len(), *t - seg_start));
            body.push(F::JumpIfFalse(0));
        }
        // strykelang `int(x)` -> fusevm's integer-producing `Op::TruncInt`.
        _ if is_trunc_int_builtin(op) => body.push(F::TruncInt),
        // strykelang `ceil(x)`/`ceiling(x)` -> [CeilFloat, TruncInt] (float ceil,
        // then truncate-to-int; ceil of a float lands on an integer value so the
        // truncation is exact). Returns Int.
        _ if is_ceil_int_builtin(op) => {
            body.push(F::CeilFloat);
            body.push(F::TruncInt);
        }
        // strykelang `floor(x)` -> [FloorFloat, TruncInt] (float floor, then
        // truncate-to-int; same exactness reasoning as ceil). Returns Int.
        _ if is_floor_int_builtin(op) => {
            body.push(F::FloorFloat);
            body.push(F::TruncInt);
        }
        // `length`/`ord`/`hex`/`oct` (→int) and `uc`/`lc`/`ucfirst`/`lcfirst`
        // (→owned string handle) ($s) -> unary host-helper-backed Extended op; the
        // operand is a raw StrykeValue bit-handle (see
        // `segment_is_string_unary_eligible`).
        _ if unary_str_ext_op(op).is_some() => {
            body.push(F::Extended(unary_str_ext_op(op).unwrap(), 0))
        }
        // `defined($x)` — unary any-value→int Extended op; the operand is a raw
        // StrykeValue bit-handle (see `segment_is_any_value_unary_int_eligible`).
        _ if unary_any_int_ext_op(op).is_some() => {
            body.push(F::Extended(unary_any_int_ext_op(op).unwrap(), 0))
        }
        // `ref($x)` — unary any-value→string-handle Extended op; same shape,
        // result is the raw bits of an owned string (see
        // `segment_is_any_value_unary_str_eligible`).
        _ if unary_any_str_ext_op(op).is_some() => {
            body.push(F::Extended(unary_any_str_ext_op(op).unwrap(), 0))
        }
        // `chr($n)` -> int→string host-helper Extended op; the operand is an unboxed
        // integer codepoint (see `segment_is_int_to_string_eligible`).
        _ if int_str_ext_op(op).is_some() => {
            body.push(F::Extended(int_str_ext_op(op).unwrap(), 0))
        }
        // 2-arg `index`/`rindex` -> binary string→int host-helper Extended op; the two
        // operands are raw StrykeValue bit-handles (see
        // `segment_is_string_binary_int_eligible`).
        _ if binary_str_int_ext_op(op).is_some() => {
            body.push(F::Extended(binary_str_int_ext_op(op).unwrap(), 0))
        }
        // 2-arg `crypt` -> binary string→string host-helper Extended op; both
        // operands are raw StrykeValue bit-handles, result is an owned string
        // handle (see `segment_is_string_binary_str_eligible`).
        _ if binary_str_str_ext_op(op).is_some() => {
            body.push(F::Extended(binary_str_str_ext_op(op).unwrap(), 0))
        }
        // `substr($s,$off)` (2-arg) / `$s x $n` -> binary string+integer→string
        // host-helper Extended op; operand `a` is a string handle, operand `b` an
        // unboxed integer (see `segment_is_string_int_to_string_eligible`).
        _ if str_int_str_ext_op(op).is_some() => {
            body.push(F::Extended(str_int_str_ext_op(op).unwrap(), 0))
        }
        // `substr($s,$off,$len)` (3-arg) -> ternary string+int+int→string host-helper
        // Extended op (see `segment_is_string_int2_to_string_eligible`).
        _ if str_int2_str_ext_op(op).is_some() => {
            body.push(F::Extended(str_int2_str_ext_op(op).unwrap(), 0))
        }
        // Always-float math built-in (`sqrt`/`sin`/`cos`/`exp`/`atan2`) -> the
        // corresponding native fusevm float op, which JITs and persists to the
        // on-disk cache.
        _ if is_float_builtin(op) => {
            body.push(float_builtin_fusevm_op(op).expect("is_float_builtin implies a lowering"))
        }
        _ => return false,
    }
    true
}

/// Emit `$d = $d <arith> $s` followed by a reload of the new value (so the
/// op leaves its result on the stack, matching strykelang's push-variant
/// `*AssignSlotSlot` ops).
#[inline]
fn emit_slot_assign(body: &mut Vec<fusevm::Op>, d: u8, s: u8, arith: fusevm::Op) {
    use fusevm::Op as F;
    body.push(F::GetSlot(d as u16));
    body.push(F::GetSlot(s as u16));
    body.push(arith);
    body.push(F::SetSlot(d as u16));
    body.push(F::GetSlot(d as u16));
}

/// Build the fusevm chunk for `seg`: a `PushFrame` + slot-init preamble seeded
/// from `slot_buf`, followed by the translated body. Because some strykelang ops
/// expand to several fusevm ops, jump targets are remapped through a per-source-op
/// offset table rather than a fixed 1:1 stride.
fn build_chunk(
    seg: &[Op],
    seg_start: usize,
    slot_buf: &[i64],
    seed_slots: bool,
    constants: &[StrykeValue],
) -> Option<fusevm::Chunk> {
    let preamble = preamble_len(slot_buf.len(), seed_slots);

    // Pass 1: translate the body, recording where each source op starts and
    // any jump ops that need their targets resolved.
    let mut body: Vec<fusevm::Op> = Vec::with_capacity(seg.len());
    let mut src_off: Vec<usize> = Vec::with_capacity(seg.len() + 1);
    let mut fixups: Vec<(usize, usize)> = Vec::new();
    for op in seg {
        src_off.push(body.len());
        if !translate_op_into(op, seg_start, &mut body, &mut fixups, constants) {
            return None;
        }
    }
    // A jump to `seg_end` (one past the last op) lands here.
    src_off.push(body.len());

    // Pass 2: resolve jump targets to absolute chunk positions.
    for (body_idx, src_target) in fixups {
        let target = preamble + src_off.get(src_target).copied()?;
        match &mut body[body_idx] {
            fusevm::Op::Jump(t) | fusevm::Op::JumpIfTrue(t) | fusevm::Op::JumpIfFalse(t) => {
                *t = target;
            }
            fusevm::Op::SlotLtIntJumpIfFalse(_, _, t)
            | fusevm::Op::SlotIncLtIntJumpBack(_, _, t) => {
                *t = target;
            }
            _ => {}
        }
    }

    let mut b = fusevm::ChunkBuilder::new();
    b.emit(fusevm::Op::PushFrame, 0);
    // Integer segments bake their marshaled slot seeds into the chunk (stable
    // because eligible slots are write-before-read, seeded 0). String-comparison
    // segments must NOT bake their operand handles (live pointers vary per run and
    // would both poison the disk-cache key and persist a stale pointer into cached
    // native code); the block JIT reads them from the runtime `slots` buffer.
    if seed_slots {
        for (i, v) in slot_buf.iter().enumerate() {
            b.emit(fusevm::Op::LoadInt(*v), 0);
            b.emit(fusevm::Op::SetSlot(i as u16), 0);
        }
    }
    for op in body {
        b.emit(op, 0);
    }
    Some(b.build())
}

/// Execute an eligible linear segment on `fusevm::VM`.
///
/// `slot_buf` is pre-filled by the caller with the integer values of the slots
/// the segment reads; on success it is overwritten in place with the slots'
/// post-execution values (caller writes the changed ones back into scope).
///
/// Returns `Some(return_value)` when the segment is eligible and fusevm ran it
/// (the value is the body's top-of-stack result, used by a `ReturnValue`
/// terminator); returns `None` when the segment is ineligible or fusevm errored,
/// in which case the caller falls back to strykelang's own JIT/interpreter and
/// `slot_buf` must be treated as unchanged.
/// Whether `seg`'s computed result is always an integer/bool — i.e. it contains
/// no `Div`/`Pow`, the only eligible ops that yield a `fusevm::Value::Float`.
///
/// fusevm's block JIT returns an `i64`, so only integer-result segments may take
/// the JIT path; float-result segments fall back to the interpreter, which
/// preserves the float through [`value_from_fusevm`].
#[inline]
fn segment_result_is_integer(seg: &[Op]) -> bool {
    !seg.iter()
        .any(|o| matches!(o, Op::Div | Op::Pow) || is_float_builtin(o))
}

/// Whether `op` is a strykelang unary math built-in call (`sqrt`/`sin`/`cos`/
/// `exp`) that the bridge lowers to an always-float fusevm op. Such a segment
/// yields a float regardless of its operand kinds, so — like `Div`/`Pow` — it
/// routes through the float block-JIT path (see
/// [`segment_fusevm_float_result_kind`]) rather than the integer one, and its
/// result is reconstructed as a float.
#[inline]
fn is_float_unary_builtin(op: &Op) -> bool {
    use crate::bytecode::BuiltinId;
    matches!(op, Op::CallBuiltin(id, 1)
        if *id == BuiltinId::Sqrt as u16
            || *id == BuiltinId::Sin as u16
            || *id == BuiltinId::Cos as u16
            || *id == BuiltinId::Exp as u16
            || *id == BuiltinId::Log as u16
            || *id == BuiltinId::Abs as u16
            || *id == BuiltinId::Tan as u16
            || *id == BuiltinId::Asin as u16
            || *id == BuiltinId::Acos as u16
            || *id == BuiltinId::Atan as u16
            || *id == BuiltinId::Sinh as u16
            || *id == BuiltinId::Cosh as u16
            || *id == BuiltinId::Tanh as u16
            || *id == BuiltinId::Log2 as u16
            || *id == BuiltinId::Log10 as u16)
}

/// Whether `op` is a strykelang binary math built-in call (`atan2`) that the
/// bridge lowers to an always-float fusevm op. See [`is_float_unary_builtin`].
#[inline]
fn is_float_binary_builtin(op: &Op) -> bool {
    matches!(op, Op::CallBuiltin(id, 2) if *id == crate::bytecode::BuiltinId::Atan2 as u16)
}

/// Whether `op` is any always-float math built-in the bridge lowers (unary or
/// binary). Segments containing one always produce a float.
#[inline]
fn is_float_builtin(op: &Op) -> bool {
    is_float_unary_builtin(op) || is_float_binary_builtin(op)
}

/// Whether `op` is strykelang's `int(x)` built-in, which the bridge lowers to
/// fusevm's integer-producing [`fusevm::Op::TruncInt`] (truncate toward zero).
/// Unlike the always-float built-ins, `int` yields an *integer*, so a segment
/// containing it can take the integer block-JIT path; crucially it also turns an
/// otherwise-unreturnable float (e.g. `int($x / 2)`) into a returnable integer.
#[inline]
fn is_trunc_int_builtin(op: &Op) -> bool {
    matches!(op, Op::CallBuiltin(id, 1) if *id == crate::bytecode::BuiltinId::Int as u16)
}

/// Whether `op` is `ceil(x)`/`ceiling(x)` — float operand, INTEGER result. Lowered
/// to the 2-op fusevm sequence `[CeilFloat, TruncInt]`. Stack effect: pop 1 push 1
/// (net 0, like `int()`). Lets a float-bearing segment (e.g. `ceil($x / 2)`) yield
/// a block-JIT-returnable integer.
#[inline]
fn is_ceil_int_builtin(op: &Op) -> bool {
    matches!(op, Op::CallBuiltin(id, 1) if *id == crate::bytecode::BuiltinId::Ceil as u16)
}

/// Whether `op` is `floor(x)` — float operand, INTEGER result. Lowered to
/// `[FloorFloat, TruncInt]`. See [`is_ceil_int_builtin`].
#[inline]
fn is_floor_int_builtin(op: &Op) -> bool {
    matches!(op, Op::CallBuiltin(id, 1) if *id == crate::bytecode::BuiltinId::Floor as u16)
}

/// Whether `op` is any int-returning float-operand builtin lowered to fusevm:
/// `int(x)`, `ceil(x)`, `floor(x)`. Same eligibility/stack/emit story.
#[inline]
fn is_int_returning_float_builtin(op: &Op) -> bool {
    is_trunc_int_builtin(op) || is_ceil_int_builtin(op) || is_floor_int_builtin(op)
}

/// The fusevm always-float op a lowerable math built-in maps to, or `None` if
/// `op` is not such a built-in.
#[inline]
fn float_builtin_fusevm_op(op: &Op) -> Option<fusevm::Op> {
    use crate::bytecode::BuiltinId;
    use fusevm::Op as F;
    let Op::CallBuiltin(id, _) = op else {
        return None;
    };
    let id = *id;
    if id == BuiltinId::Sqrt as u16 {
        Some(F::SqrtFloat)
    } else if id == BuiltinId::Sin as u16 {
        Some(F::SinFloat)
    } else if id == BuiltinId::Cos as u16 {
        Some(F::CosFloat)
    } else if id == BuiltinId::Exp as u16 {
        Some(F::ExpFloat)
    } else if id == BuiltinId::Log as u16 {
        Some(F::LogFloat)
    } else if id == BuiltinId::Abs as u16 {
        Some(F::AbsFloat)
    } else if id == BuiltinId::Atan2 as u16 {
        Some(F::Atan2Float)
    } else if id == BuiltinId::Tan as u16 {
        Some(F::TanFloat)
    } else if id == BuiltinId::Asin as u16 {
        Some(F::AsinFloat)
    } else if id == BuiltinId::Acos as u16 {
        Some(F::AcosFloat)
    } else if id == BuiltinId::Atan as u16 {
        Some(F::AtanFloat)
    } else if id == BuiltinId::Sinh as u16 {
        Some(F::SinhFloat)
    } else if id == BuiltinId::Cosh as u16 {
        Some(F::CoshFloat)
    } else if id == BuiltinId::Tanh as u16 {
        Some(F::TanhFloat)
    } else if id == BuiltinId::Log2 as u16 {
        Some(F::Log2Float)
    } else if id == BuiltinId::Log10 as u16 {
        Some(F::Log10Float)
    } else {
        None
    }
}

/// Configure fusevm's per-thread block-JIT warmup so the first invocation of a
/// chunk compiles (or loads from the on-disk cache) immediately, instead of
/// interpreting once to warm up. strykelang's "re-run the same script" workload
/// runs each top-level segment once per process, so eager compilation is what
/// makes the disk cache populate and subsequent runs skip codegen.
///
/// Respects an explicit `FUSEVM_JIT_BLOCK_THRESHOLD` override if the user set one.
fn configure_block_jit_eager() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    DONE.with(|d| {
        if d.get() {
            return;
        }
        if std::env::var_os("FUSEVM_JIT_BLOCK_THRESHOLD").is_none() {
            let jit = fusevm::JitCompiler::new();
            let mut cfg = jit.get_config();
            cfg.block_threshold = 0;
            jit.set_config(cfg);
        }
        register_stryke_jit_ext();
        d.set(true);
    });
}

pub(crate) fn run_linear_segment(
    seg: &[Op],
    seg_start: usize,
    slot_buf: &mut [i64],
    term: SubTerminator,
    constants: &[StrykeValue],
) -> Option<StrykeValue> {
    if !matches!(term, SubTerminator::Value) {
        return None;
    }
    // Publish the chunk-constants slice to the thread-local context for the
    // duration of any `try_run_block` invocation below, so a `LoadConst(idx)`
    // operand translated to [`STK_VAL_LOAD_CONST`] can resolve to the right
    // `StrykeValue` at runtime. The guard restores the prior context on drop
    // (so nested calls correctly stack). Setting this *unconditionally* (vs
    // per-branch) keeps the future addition of LoadConst-bearing eligibility
    // families a one-line change.
    let _load_const_ctx = LoadConstCtxGuard::new(constants);
    let int_ok = segment_is_fusevm_eligible(seg, seg_start);
    // The float-operand path also reports the segment's *static* result kind
    // (`Int` for a comparison/bool, `Float` for arithmetic involving a float). We
    // trust this strykelang-side prediction to decide how to reconstruct the
    // result, because fusevm's block JIT collapses whole-number float literals to
    // integers (e.g. `$x * 2.0` is computed in i64), so its runtime kind would
    // under-report floats. Integer arithmetic over whole-number operands is
    // bit-identical to the float computation, so coercing such an integer result
    // to a float is exact.
    //
    // A segment can be `int_ok` yet still yield a float when it contains an
    // always-float op (`Div`/`Pow`, lowered to fusevm `Op::AwkDivJit`/`PowFloat`):
    // `segment_result_is_integer` reports it as *not* integer-result, so we must
    // still compute `float_kind` for it — otherwise such a bare segment (e.g.
    // `$x / $y` or `$x ** $y`) would skip BOTH the integer block JIT (wrong
    // result kind) and the float block JIT, falling to the interpreter and never
    // persisting a native blob to the on-disk cache.
    let float_kind = if int_ok && segment_result_is_integer(seg) {
        None
    } else {
        segment_fusevm_float_result_kind(seg, seg_start)
    };
    let float_ok = float_kind.is_some();
    let concat_ok =
        !int_ok && !float_ok && segment_is_string_concat_eligible(seg, seg_start);
    let str_ok = !int_ok
        && !float_ok
        && !concat_ok
        && (segment_is_string_compare_eligible(seg, seg_start)
            || segment_is_string_binary_int_eligible(seg, seg_start));
    // Binary string→**string** builtins (currently just 2-arg `crypt`). Result is
    // an owned string handle reconstructed via `from_raw_bits`, like concat.
    let str_str_bin_ok = !int_ok
        && !float_ok
        && !concat_ok
        && !str_ok
        && segment_is_string_binary_str_eligible(seg, seg_start);
    let unary_ok = !int_ok
        && !float_ok
        && !concat_ok
        && !str_ok
        && !str_str_bin_ok
        && (segment_is_string_unary_eligible(seg, seg_start)
            // `length($s) > N`, `length($s) - 1`, `ord($c) >= 0x80` etc. —
            // unary-str-int builtin combined with an int literal via a binary
            // int op. Same seeder (slot as string handle) and result (plain i64)
            // as the bare unary case, so it joins the same dispatch branch.
            || segment_is_string_unary_int_combined_eligible(seg, seg_start)
            // `length($a) > length($b)`, `length($a) - length($b)`,
            // `ord($a) == ord($b)` — two unary-str-int builtins on two slots
            // combined via a binary int op. Both slots marshal as string
            // handles (str_ok seeder default) and the result is a plain i64.
            || segment_is_string_unary_binop_eligible(seg, seg_start));
    // Subset of `unary_ok` whose JIT result is an owned string handle (uc/lc/…),
    // reconstructed like concatenation rather than boxed as an integer.
    let unary_str_ok = unary_ok && segment_is_string_unary_str_eligible(seg, seg_start);
    // Unary any-value→int (`defined($x)`): same shape as `unary_ok` but the
    // operand can be ANY type — the seeder (in `vm.rs`) bypasses `is_string_like`.
    // Result is a plain `i64`.
    let val_unary_int_ok = !int_ok
        && !float_ok
        && !concat_ok
        && !str_ok
        && !str_str_bin_ok
        && !unary_ok
        && segment_is_any_value_unary_int_eligible(seg, seg_start);
    // Unary any-value→string handle (`ref($x)`): same any-value seeder as
    // `val_unary_int_ok` (bypass `is_string_like`), but the result is an
    // owned string handle reconstructed via `from_raw_bits`.
    let val_unary_str_ok = !int_ok
        && !float_ok
        && !concat_ok
        && !str_ok
        && !str_str_bin_ok
        && !unary_ok
        && !val_unary_int_ok
        && segment_is_any_value_unary_str_eligible(seg, seg_start);
    // `chr($n)`: integer operand (marshaled unboxed by the caller, exactly like the
    // integer fast path), owned-string-handle result (reconstructed via from_raw_bits).
    let int_str_ok = !int_ok
        && !float_ok
        && !str_ok
        && !concat_ok
        && !unary_ok
        && segment_is_int_to_string_eligible(seg, seg_start);
    // `substr($s,$off)` (2-arg) / `$s x $n`: a string operand + an integer operand
    // (marshaled per-slot by the caller), owned-string-handle result.
    let str_int_ok = !int_ok
        && !float_ok
        && !str_ok
        && !concat_ok
        && !unary_ok
        && !int_str_ok
        && segment_is_string_int_mixed_eligible(seg, seg_start);
    // `substr("abc", $n)` / `"prefix" x $n`: literal string + slot int → string.
    // No slot is a string handle (the literal supplies the string via
    // `STK_VAL_LOAD_CONST`), so the single slot here marshals as a plain integer
    // — identical seeding to `int_str_ok` for `chr($n)`. Result is an owned
    // string handle, reconstructed via `from_raw_bits`.
    let lit_str_int_ok = !int_ok
        && !float_ok
        && !str_ok
        && !concat_ok
        && !unary_ok
        && !int_str_ok
        && !str_int_ok
        && segment_is_literal_string_int_to_string_eligible(seg, seg_start);
    if !int_ok
        && !float_ok
        && !str_ok
        && !concat_ok
        && !unary_ok
        && !int_str_ok
        && !str_int_ok
        && !lit_str_int_ok
        && !str_str_bin_ok
        && !val_unary_int_ok
        && !val_unary_str_ok
    {
        return None;
    }

    // String segments (comparison + concatenation + unary length/ord/hex/oct + the
    // int→string `chr`): build an *unseeded* chunk (operand values flow through the
    // runtime `slots` buffer, never baked into the chunk) and run it strictly on the
    // block JIT, which is configured to compile eagerly. There is no fusevm-interpreter
    // fallback here: that path reads slots from the VM frame, which an unseeded chunk
    // never populates. On block-JIT decline we return None so strykelang falls back to
    // its own interpreter.
    //
    // Comparison and unary length/ord/hex/oct results are plain `i64` outcomes.
    // Concatenation and the string-producing ops (uc/lc/ucfirst/lcfirst, chr) return
    // the raw bits of a freshly allocated, *owned* string handle, which we reconstitute
    // into exactly one owning `StrykeValue` via `from_raw_bits` (the helper
    // `mem::forget`-ed it, transferring ownership).
    if str_ok || concat_ok || unary_ok || int_str_ok || str_int_ok || lit_str_int_ok
        || str_str_bin_ok || val_unary_int_ok || val_unary_str_ok
    {
        configure_block_jit_eager();
        let chunk = build_chunk(seg, seg_start, slot_buf, false, constants)?;
        let jit = fusevm::JitCompiler::new();
        return jit.try_run_block(&chunk, slot_buf).map(|ret| {
            // val_unary_int_ok returns a plain integer (defined: 0 or 1).
            // val_unary_str_ok returns the raw bits of an owned string handle
            // (ref: "ARRAY"/"HASH"/etc), reconstructed via from_raw_bits.
            // lit_str_int_ok returns an owned string handle (the result of
            // substr/repeat applied to a literal-string operand).
            if concat_ok || unary_str_ok || int_str_ok || str_int_ok || lit_str_int_ok
                || str_str_bin_ok || val_unary_str_ok
            {
                StrykeValue::from_raw_bits(ret as u64)
            } else {
                StrykeValue::integer(ret)
            }
        });
    }

    // Fast path: pure-integer segments — and segments that compute with float
    // *operands* but yield an integer/bool result (`float_ok`, e.g. `$x < 0.5` or a
    // ternary `$x < 0.5 ? a : b`) — run on fusevm's block JIT, which persists native
    // code to the on-disk cache so repeated runs skip Cranelift codegen. The chunk is
    // built *unseeded* (just like the string segments): the marshaled operand/local
    // values flow through the runtime `slot_buf` that `try_run_block` reads via
    // `GetSlot`, instead of being baked into the chunk as `LoadInt` immediates. This
    // keeps the chunk's `op_hash` (the disk-cache key) independent of the actual
    // argument values, so a hot sub called with thousands of distinct arguments
    // reuses a SINGLE cached native blob rather than spilling one file per argument
    // combination.
    if float_ok || (int_ok && segment_result_is_integer(seg)) {
        configure_block_jit_eager();
        let chunk = build_chunk(seg, seg_start, slot_buf, false, constants)?;
        let jit = fusevm::JitCompiler::new();
        // A segment containing strykelang `/` lowers to fusevm `Op::AwkDivJit`,
        // which traps (via a thread-local flag) on a zero divisor and returns a
        // sentinel. On a trap we must discard the JIT result and decline so
        // strykelang's interpreter re-runs and raises its own "Illegal division by
        // zero". Clear any stale flag before the run, then check it after.
        let has_div = seg.iter().any(|o| matches!(o, Op::Div));
        if has_div {
            jit.take_awk_div_trap();
        }
        match float_kind {
            // Float-result segment (e.g. `$x * 1.5` or `$x / $y`): use the typed
            // entry point so a genuinely fractional result comes back exactly.
            // fusevm may still report a whole-number result as `Int` (whole-float-
            // literal collapse); coerce it to a float since the static analysis
            // proved the value is a float.
            Some(NumTy::Float) => {
                if let Some(num) = jit.try_run_block_typed_kinded(&chunk, slot_buf, &[]) {
                    if has_div && jit.take_awk_div_trap() {
                        return None;
                    }
                    return Some(match num {
                        fusevm::BlockNum::Float(f) => StrykeValue::float(f),
                        fusevm::BlockNum::Int(n) => StrykeValue::float(n as f64),
                    });
                }
            }
            // Integer/bool-result segment (e.g. a float comparison `$x < 0.5`, or a
            // division feeding a comparison `$x / $y < 1`): the result is a genuine
            // integer, so the plain i64 entry point is exact.
            Some(NumTy::Int) | None => {
                if let Some(ret) = jit.try_run_block(&chunk, slot_buf) {
                    if has_div && jit.take_awk_div_trap() {
                        return None;
                    }
                    return Some(StrykeValue::integer(ret));
                }
            }
        }
    }

    // Fallback: fusevm's interpreter. Handles float/bool results and any chunk the
    // block JIT declines, reading the post-run slot values from the VM frame. This
    // path *does* need the slots seeded into the frame, so it uses a seeded chunk
    // (not disk-cached as native code, so the per-value `op_hash` churn is moot).
    // The strykelang Extended handler makes `%` (STK_MOD_FLOOR) correct on this
    // cold path too; without it fusevm would treat the unknown op as a no-op.
    let chunk = build_chunk(seg, seg_start, slot_buf, true, constants)?;
    let mut vm = fusevm::VM::new(chunk);
    vm.set_extension_handler(Box::new(stryke_ext_handler));
    let result = vm.run();

    if let Some(frame) = vm.frames.last() {
        for (i, slot) in slot_buf.iter_mut().enumerate() {
            if let Some(v) = frame.slots.get(i) {
                *slot = v.to_int();
            }
        }
    }

    match result {
        fusevm::VMResult::Ok(val) => Some(value_from_fusevm(&val)),
        _ => None,
    }
}

/// Convert a `fusevm::Value` produced by the universal-integer subset back into a
/// `StrykeValue`. The subset only yields integers/bools/floats.
#[inline]
fn value_from_fusevm(v: &fusevm::Value) -> StrykeValue {
    match v {
        fusevm::Value::Int(n) => StrykeValue::integer(*n),
        fusevm::Value::Bool(b) => StrykeValue::integer(if *b { 1 } else { 0 }),
        fusevm::Value::Float(f) => StrykeValue::float(*f),
        other => StrykeValue::integer(other.to_int()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Op;

    // `Op::Mod` is now eligible: it lowers to `Extended(STK_MOD_FLOOR)` and runs
    // on fusevm with Perl *floored* semantics (sign of divisor). Cover both signs
    // and the divide-trap guards (b == 0 and b == -1).
    #[test]
    fn fusevm_runs_floored_modulo() {
        let cases = [
            (-7_i64, 3_i64, 2_i64),
            (7, 3, 1),
            (7, -3, -2),
            (-7, -3, -1),
            (-6, 3, 0),
            (10, 4, 2),
            (5, 0, 0),    // guarded: no trap
            (-8, -1, 0),  // guarded: no trap
        ];
        for (a, b, want) in cases {
            let seg = vec![Op::LoadInt(a), Op::LoadInt(b), Op::Mod];
            assert!(segment_is_fusevm_eligible(&seg, 0));
            let mut slots: [i64; 0] = [];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                .expect("modulo segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "{a} % {b}");
        }
    }

    // A ternary `?:` body computes its result in each branch and merges it on the
    // operand stack — at an interior join or via a value-carrying jump to the implicit
    // return point. fusevm's block JIT now carries such cross-block operand-stack values
    // as block parameters, so these segments are eligible (and disk-cacheable). The
    // stack-consistency check still rejects genuinely malformed segments (inconsistent
    // merge depth / underflow). A straight-line body and a counted loop stay eligible.
    #[test]
    fn ternary_merge_segment_is_eligible() {
        // `$neg ? 2*$v+1 : 2*$v` — JumpIfFalse to the else branch, then Jump to the
        // segment end carrying the result on the stack (both edges merge at depth 1).
        let ternary = vec![
            Op::GetScalarSlot(0),  // 0: $neg
            Op::JumpIfFalse(8),    // 1: -> else (index 8)
            Op::LoadInt(2),        // 2
            Op::GetScalarSlot(1),  // 3: $v
            Op::Mul,               // 4
            Op::LoadInt(1),        // 5
            Op::Add,               // 6
            Op::Jump(11),          // 7: -> end (len == 11)
            Op::LoadInt(2),        // 8: else
            Op::GetScalarSlot(1),  // 9: $v
            Op::Mul,               // 10
        ];
        assert!(segment_block_stack_is_consistent(&ternary, 0));
        assert!(segment_is_fusevm_eligible(&ternary, 0));

        // Straight-line arithmetic is fine.
        let linear = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), Op::Add];
        assert!(segment_block_stack_is_consistent(&linear, 0));
        assert!(segment_is_fusevm_eligible(&linear, 0));

        // A fused counted loop reaches every leader with an empty stack (state is in
        // slots), so it stays eligible.
        let loop_seg = vec![
            Op::SlotLtIntJumpIfFalse(1, 100, 4), // 0: top: while i<100 -> exit(4)
            Op::PreIncSlotVoid(0),               // 1: body
            Op::SlotIncLtIntJumpBack(1, 100, 1), // 2: i++; if i<100 -> body(1)
            Op::Jump(4),                         // 3: -> exit(4)
            Op::GetScalarSlot(0),                // 4: exit: push result
        ];
        assert!(segment_block_stack_is_consistent(&loop_seg, 0));

        // An inconsistent merge — a leader reached at two different stack depths —
        // is still rejected.
        let inconsistent = vec![
            Op::GetScalarSlot(0), // 0: depth -> 1
            Op::JumpIfFalse(3),   // 1: pops -> 0; jump to 3 (depth 0) / fall to 2 (depth 0)
            Op::LoadInt(1),       // 2: depth -> 1; falls to 3 at depth 1
            Op::GetScalarSlot(1), // 3: reached at depth 0 (jump) AND depth 1 (fall) -> bad
        ];
        assert!(!segment_block_stack_is_consistent(&inconsistent, 0));
    }

    // A float comparison such as `$x < 0.5` computes in floating point (fusevm
    // promotes the integer slot to f64) but yields an integer/bool result, which the
    // block JIT can return as an exact i64. These segments are now eligible via the
    // float-operand path and produce the same 0/1 the interpreter would.
    #[test]
    fn float_compare_segment_runs_on_fusevm() {
        // `$x < 1.5` for an integer slot $x: true for 1, false for 2.
        let seg = vec![Op::GetScalarSlot(0), Op::LoadFloat(1.5), Op::NumLt];
        assert!(!segment_is_fusevm_eligible(&seg, 0));
        assert!(segment_is_fusevm_float_eligible(&seg, 0));

        for (x, want) in [(1_i64, 1_i64), (2, 0), (-3, 1)] {
            let mut slots = [x];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-compare segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "{x} < 1.5");
        }

        // A ternary whose float condition selects between two integers merges an
        // integer on both edges, so it stays eligible: `$x < 1.5 ? 10 : 20`.
        let ternary = vec![
            Op::GetScalarSlot(0), // 0: $x
            Op::LoadFloat(1.5),   // 1
            Op::NumLt,            // 2
            Op::JumpIfFalse(6),   // 3: -> else (index 6)
            Op::LoadInt(10),      // 4
            Op::Jump(7),          // 5: -> end (len == 7)
            Op::LoadInt(20),      // 6: else
        ];
        assert!(segment_is_fusevm_float_eligible(&ternary, 0));
        for (x, want) in [(1_i64, 10_i64), (2, 20)] {
            let mut slots = [x];
            let out = run_linear_segment(&ternary, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-ternary segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "$x={x} ? 10 : 20");
        }

        // A float arithmetic result (`$x * 1.5`) is now eligible: fusevm's block
        // tier returns the float exactly (as f64 bits via the typed entry point),
        // so we reconstruct the precise value instead of truncating.
        let float_result = vec![Op::GetScalarSlot(0), Op::LoadFloat(1.5), Op::Mul];
        assert!(segment_is_fusevm_float_eligible(&float_result, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_result, 0),
            Some(NumTy::Float)
        );
        for x in [2_i64, 4, -3] {
            let mut slots = [x];
            let out = run_linear_segment(&float_result, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-result segment must run on fusevm");
            assert_eq!(out.as_float(), Some(x as f64 * 1.5), "$x={x} * 1.5");
        }

        // A ternary whose branches yield a float result is also eligible now and
        // returns the selected float exactly.
        let float_ternary = vec![
            Op::GetScalarSlot(0),
            Op::LoadFloat(1.5),
            Op::NumLt,
            Op::JumpIfFalse(6),
            Op::LoadFloat(1.0),
            Op::Jump(7),
            Op::LoadFloat(2.0),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_ternary, 0));
        for (x, want) in [(1_i64, 1.0_f64), (2, 2.0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_ternary, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-ternary segment must run on fusevm");
            assert_eq!(out.as_float(), Some(want), "$x={x} ? 1.0 : 2.0");
        }

        // Storing a float into an integer-marshaled slot is rejected.
        let float_to_slot =
            vec![Op::LoadFloat(2.5), Op::SetScalarSlot(0), Op::LoadInt(0)];
        assert!(!segment_is_fusevm_float_eligible(&float_to_slot, 0));

        // Float division is now eligible: strykelang `/` lowers to fusevm
        // `Op::AwkDivJit`, which divides in floating point (operands coerced to
        // f64) consistently in the block JIT, the interpreter, and the native disk
        // cache. `($x / 2.0) < 1.0` yields a bool: true for x=1 (0.5 < 1.0), false
        // for x=3 (1.5 < 1.0).
        let float_div_cmp = vec![Op::GetScalarSlot(0), Op::LoadFloat(2.0), Op::Div, Op::LoadFloat(1.0), Op::NumLt];
        assert!(segment_is_fusevm_float_eligible(&float_div_cmp, 0));
        for (x, want) in [(1_i64, 1_i64), (3, 0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_div_cmp, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-division compare must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "($x={x} / 2.0) < 1.0");
        }

        // A bare float division yields an exact float result (7 / 2 == 3.5), and is
        // eligible even without an explicit `LoadFloat` (division is always float).
        let float_div = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), Op::Div];
        assert!(segment_is_fusevm_float_eligible(&float_div, 0));
        let mut slots = [7_i64, 2_i64];
        let out = run_linear_segment(&float_div, 0, &mut slots, SubTerminator::Value, &[])
            .expect("bare float division must run on fusevm");
        assert_eq!(out.as_float(), Some(3.5), "7 / 2 == 3.5");

        // Division by zero: the JIT traps and the bridge declines (returns None) so
        // the caller's interpreter can raise strykelang's own "Illegal division by
        // zero", rather than silently returning the AwkDivJit sentinel.
        let mut zslots = [7_i64, 0_i64];
        let zout = run_linear_segment(&float_div, 0, &mut zslots, SubTerminator::Value, &[]);
        assert!(zout.is_none(), "division by zero must decline to the interpreter");

        // Always-float exponentiation: strykelang `**` lowers to fusevm
        // `Op::PowFloat`, which raises to a power in floating point (operands
        // coerced to f64 via the `pow_f64` host helper) consistently in the block
        // JIT, the interpreter, and the native disk cache. A bare `$x ** $y`
        // yields an exact float (2 ** 10 == 1024.0) and is eligible even without
        // an explicit `LoadFloat` (exponentiation is always float).
        let float_pow = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), Op::Pow];
        assert!(segment_is_fusevm_float_eligible(&float_pow, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_pow, 0),
            Some(NumTy::Float)
        );
        let mut pslots = [2_i64, 10_i64];
        let out = run_linear_segment(&float_pow, 0, &mut pslots, SubTerminator::Value, &[])
            .expect("bare float exponentiation must run on fusevm");
        assert_eq!(out.as_float(), Some(1024.0), "2 ** 10 == 1024.0");

        // `($x ** 2.0) < 5.0` yields a bool: true for x=2 (4.0 < 5.0), false for
        // x=3 (9.0 < 5.0).
        let float_pow_cmp = vec![
            Op::GetScalarSlot(0),
            Op::LoadFloat(2.0),
            Op::Pow,
            Op::LoadFloat(5.0),
            Op::NumLt,
        ];
        assert!(segment_is_fusevm_float_eligible(&float_pow_cmp, 0));
        for (x, want) in [(2_i64, 1_i64), (3, 0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_pow_cmp, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-exponentiation compare must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "($x={x} ** 2.0) < 5.0");
        }

        // `Op::CallBuiltin(Sqrt, 1)` lowers to fusevm's native `Op::SqrtFloat`
        // (Cranelift `fsqrt`), an always-float unary built-in: `sqrt($x)` yields
        // an exact float (sqrt(9) == 3.0) and is eligible even without an explicit
        // `LoadFloat`.
        let float_sqrt = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(crate::bytecode::BuiltinId::Sqrt as u16, 1),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_sqrt, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_sqrt, 0),
            Some(NumTy::Float)
        );
        let mut sslots = [9_i64];
        let out = run_linear_segment(&float_sqrt, 0, &mut sslots, SubTerminator::Value, &[])
            .expect("bare float sqrt must run on fusevm");
        assert_eq!(out.as_float(), Some(3.0), "sqrt(9) == 3.0");

        // `sqrt($x) < 2.0` yields a bool: true for x=2 (1.41 < 2.0), false for
        // x=9 (3.0 < 2.0).
        let float_sqrt_cmp = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(crate::bytecode::BuiltinId::Sqrt as u16, 1),
            Op::LoadFloat(2.0),
            Op::NumLt,
        ];
        assert!(segment_is_fusevm_float_eligible(&float_sqrt_cmp, 0));
        for (x, want) in [(2_i64, 1_i64), (9, 0)] {
            let mut slots = [x];
            let out = run_linear_segment(&float_sqrt_cmp, 0, &mut slots, SubTerminator::Value, &[])
                .expect("float-sqrt compare must run on fusevm");
            assert_eq!(out.as_integer(), Some(want), "sqrt($x={x}) < 2.0");
        }

        // The other always-float math built-ins lower to their native fusevm
        // ops too: unary `sin`/`cos`/`exp` and binary `atan2`. Each yields a
        // float and is eligible without an explicit `LoadFloat`.
        use crate::bytecode::BuiltinId;
        let float_exp = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Exp as u16, 1),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_exp, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_exp, 0),
            Some(NumTy::Float)
        );
        let mut eslots = [0_i64];
        let out = run_linear_segment(&float_exp, 0, &mut eslots, SubTerminator::Value, &[])
            .expect("bare float exp must run on fusevm");
        assert_eq!(out.as_float(), Some(1.0), "exp(0) == 1.0");

        let float_cos = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Cos as u16, 1),
        ];
        let mut cslots = [0_i64];
        let out = run_linear_segment(&float_cos, 0, &mut cslots, SubTerminator::Value, &[])
            .expect("bare float cos must run on fusevm");
        assert_eq!(out.as_float(), Some(1.0), "cos(0) == 1.0");

        let float_sin = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Sin as u16, 1),
        ];
        let mut snslots = [0_i64];
        let out = run_linear_segment(&float_sin, 0, &mut snslots, SubTerminator::Value, &[])
            .expect("bare float sin must run on fusevm");
        assert_eq!(out.as_float(), Some(0.0), "sin(0) == 0.0");

        // `log($x)` with x = slot 0 (= 1): log(1.0) == 0.0 exactly.
        let float_log = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Log as u16, 1),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_log, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_log, 0),
            Some(NumTy::Float)
        );
        let mut lgslots = [1_i64];
        let out = run_linear_segment(&float_log, 0, &mut lgslots, SubTerminator::Value, &[])
            .expect("bare float log must run on fusevm");
        assert_eq!(out.as_float(), Some(0.0), "log(1) == 0.0");

        // `abs($x)` with x = slot 0 (= -5): abs(-5) == 5.0.
        let float_abs = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Abs as u16, 1),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_abs, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_abs, 0),
            Some(NumTy::Float)
        );
        let mut absslots = [-5_i64];
        let out = run_linear_segment(&float_abs, 0, &mut absslots, SubTerminator::Value, &[])
            .expect("bare float abs must run on fusevm");
        assert_eq!(out.as_float(), Some(5.0), "abs(-5) == 5.0");

        // `atan2($y, $x)`: the chunk pushes y (slot 0) then x (slot 1); atan2(0, 1) == 0.0.
        let float_atan2 = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::CallBuiltin(BuiltinId::Atan2 as u16, 2),
        ];
        assert!(segment_is_fusevm_float_eligible(&float_atan2, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&float_atan2, 0),
            Some(NumTy::Float)
        );
        let mut a2slots = [0_i64, 1_i64];
        let out = run_linear_segment(&float_atan2, 0, &mut a2slots, SubTerminator::Value, &[])
            .expect("bare float atan2 must run on fusevm");
        assert_eq!(out.as_float(), Some(0.0), "atan2(0, 1) == 0.0");

        // `int($x / 2)` with x = slot 0 (= 7): div makes the segment float-bearing,
        // but `int` truncates to a block-JIT-returnable integer. 7 / 2 == 3.5 ->
        // int == 3.
        let int_div = vec![
            Op::GetScalarSlot(0),
            Op::LoadInt(2),
            Op::Div,
            Op::CallBuiltin(BuiltinId::Int as u16, 1),
        ];
        assert!(segment_is_fusevm_float_eligible(&int_div, 0));
        assert_eq!(
            segment_fusevm_float_result_kind(&int_div, 0),
            Some(NumTy::Int),
            "int(div) yields an integer result"
        );
        let mut idslots = [7_i64];
        let out = run_linear_segment(&int_div, 0, &mut idslots, SubTerminator::Value, &[])
            .expect("int($x / 2) must run on fusevm");
        assert_eq!(out.as_integer(), Some(3), "int(7 / 2) == 3");

        // Pure `int($x)` (integer operand) takes the integer block-JIT path.
        let int_pure = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Int as u16, 1),
        ];
        assert!(segment_is_fusevm_eligible(&int_pure, 0));
        let mut ipslots = [-9_i64];
        let out = run_linear_segment(&int_pure, 0, &mut ipslots, SubTerminator::Value, &[])
            .expect("int($x) must run on fusevm");
        assert_eq!(out.as_integer(), Some(-9), "int(-9) == -9");

        // A segment with no float operand is not a float segment (the integer path
        // covers it).
        let pure_int = vec![Op::GetScalarSlot(0), Op::LoadInt(1), Op::NumLt];
        assert!(!segment_is_fusevm_float_eligible(&pure_int, 0));
    }

    // A string-compare segment that mixes in non-string ops (here `LoadInt`) is not a
    // pure string-comparison body and is rejected, regardless of control flow. (Cross-
    // block operand-stack merges are no longer themselves disqualifying — fusevm carries
    // them as block params — so the rejection here is purely the op-set gate.)
    #[test]
    fn string_compare_with_non_string_ops_is_ineligible() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::StrEq,
            Op::JumpIfFalse(7),
            Op::LoadInt(1),
            Op::Jump(8),
            Op::Pop,
            Op::LoadInt(0),
        ];
        assert!(!segment_is_string_compare_eligible(&seg, 0));
    }

    // String comparisons lower to host-helper-backed Extended ops and run on
    // fusevm's block JIT (result is i64 0/1, or -1/0/1 for cmp). Operands are the
    // raw NaN-boxed `StrykeValue` bits, marshaled as i64 slot handles; the helper
    // reconstructs them and defers to strykelang's native `str_eq`/`str_cmp`, so
    // results match the interpreter byte-for-byte (incl. multibyte UTF-8).
    #[test]
    fn fusevm_runs_string_comparisons() {
        // Keep operands alive for the duration of the call (the helper borrows the
        // Arc-backed string via its raw bits without taking ownership).
        let foo = StrykeValue::string("foo".to_string());
        let bar = StrykeValue::string("bar".to_string());
        let foo2 = StrykeValue::string("foo".to_string());
        // "café" with a multibyte grapheme vs an emoji — exercises non-ASCII bytes.
        let cafe = StrykeValue::string("café".to_string());
        let emoji = StrykeValue::string("🦀".to_string());

        // (a, b, op, expected) where op is the strykelang bytecode op.
        let cases: &[(&StrykeValue, &StrykeValue, Op, i64)] = &[
            (&foo, &foo2, Op::StrEq, 1),
            (&foo, &bar, Op::StrEq, 0),
            (&foo, &bar, Op::StrNe, 1),
            (&foo, &foo2, Op::StrNe, 0),
            (&bar, &foo, Op::StrLt, 1),  // "bar" < "foo"
            (&foo, &bar, Op::StrLt, 0),
            (&foo, &bar, Op::StrGt, 1),
            (&bar, &foo, Op::StrGt, 0),
            (&foo, &foo2, Op::StrLe, 1),
            (&foo, &bar, Op::StrLe, 0),
            (&foo, &foo2, Op::StrGe, 1),
            (&bar, &foo, Op::StrGe, 0),
            (&bar, &foo, Op::StrCmp, -1),
            (&foo, &foo2, Op::StrCmp, 0),
            (&foo, &bar, Op::StrCmp, 1),
            (&cafe, &emoji, Op::StrLt, 1), // 'c' (0x63) < first emoji byte (0xF0)
            (&cafe, &cafe, Op::StrEq, 1),
        ];
        for (a, b, op, want) in cases {
            let seg = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), op.clone()];
            assert!(
                segment_is_string_compare_eligible(&seg, 0),
                "segment must be string-compare eligible for {op:?}"
            );
            let mut slots = [a.raw_bits() as i64, b.raw_bits() as i64];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                .expect("string-compare segment must run on fusevm");
            assert_eq!(out.as_integer(), Some(*want), "op {op:?}");
        }
    }

    // `$a . $b` lowered to fusevm allocates a NEW owned string handle whose bytes
    // equal the interpreter's `push_str`, for ASCII, multibyte UTF-8 and empty
    // operands. The operands themselves must survive (they are only borrowed), and
    // running many iterations would surface any use-after-free / double-free in the
    // forget-on-return ownership transfer.
    #[test]
    fn fusevm_runs_string_concat() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Concat,
        ];
        assert!(segment_is_string_concat_eligible(&seg, 0));

        let cases: &[(&str, &str)] = &[
            ("foo", "bar"),
            ("", "tail"),
            ("head", ""),
            ("", ""),
            ("café", "🦀"),
            ("naïve", "façade"),
        ];
        for (a_s, b_s) in cases {
            let a = StrykeValue::string((*a_s).to_string());
            let b = StrykeValue::string((*b_s).to_string());
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [a.raw_bits() as i64, b.raw_bits() as i64];
                let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("string-concat segment must run on fusevm");
                last = out.as_str();
            }
            let want = format!("{a_s}{b_s}");
            assert_eq!(last.as_deref(), Some(want.as_str()), "{a_s:?} . {b_s:?}");
            // Operands are only borrowed by the helper, so they remain intact.
            assert_eq!(a.as_str().as_deref(), Some(*a_s));
            assert_eq!(b.as_str().as_deref(), Some(*b_s));
        }
    }

    // `$a . "literal"` lowered to fusevm: the literal LoadConst gets resolved at
    // runtime by [`ext_ops::STK_VAL_LOAD_CONST`] off the thread-local constants
    // pool [`LoadConstCtxGuard`] publishes, so the chunk is disk-cache safe
    // (hashes the constant *index*, not the per-process heap-pointer bits).
    // Tests both operand orderings: `slot . literal` and `literal . slot`.
    #[test]
    fn fusevm_runs_string_concat_with_load_const() {
        // `$a . " world"` shape: GetScalarSlot, LoadConst, Concat.
        let seg_right = vec![Op::GetScalarSlot(0), Op::LoadConst(0), Op::Concat];
        assert!(segment_is_string_concat_eligible(&seg_right, 0));

        // `"prefix: " . $b` shape: LoadConst, GetScalarSlot, Concat.
        let seg_left = vec![Op::LoadConst(0), Op::GetScalarSlot(0), Op::Concat];
        assert!(segment_is_string_concat_eligible(&seg_left, 0));

        let cases: &[(&str, &str)] = &[
            ("foo", "bar"),
            ("", "tail"),
            ("head", ""),
            ("café", "🦀"),
        ];

        for (slot_s, literal_s) in cases {
            let slot_v = StrykeValue::string((*slot_s).to_string());
            let literal_v = StrykeValue::string((*literal_s).to_string());
            let constants = vec![literal_v.clone()];

            // slot . literal
            let mut last_r = None;
            for _ in 0..32 {
                let mut slots = [slot_v.raw_bits() as i64];
                let out = run_linear_segment(
                    &seg_right,
                    0,
                    &mut slots,
                    SubTerminator::Value,
                    &constants,
                )
                .expect("slot+literal concat must run on fusevm");
                last_r = out.as_str();
            }
            let want_r = format!("{slot_s}{literal_s}");
            assert_eq!(last_r.as_deref(), Some(want_r.as_str()), "{slot_s:?} . {literal_s:?}");

            // literal . slot
            let mut last_l = None;
            for _ in 0..32 {
                let mut slots = [slot_v.raw_bits() as i64];
                let out = run_linear_segment(
                    &seg_left,
                    0,
                    &mut slots,
                    SubTerminator::Value,
                    &constants,
                )
                .expect("literal+slot concat must run on fusevm");
                last_l = out.as_str();
            }
            let want_l = format!("{literal_s}{slot_s}");
            assert_eq!(last_l.as_deref(), Some(want_l.as_str()), "{literal_s:?} . {slot_s:?}");

            // Operands and constants both survive (borrow-only contract).
            assert_eq!(slot_v.as_str().as_deref(), Some(*slot_s));
            assert_eq!(literal_v.as_str().as_deref(), Some(*literal_s));
            assert_eq!(constants[0].as_str().as_deref(), Some(*literal_s));
        }
    }

    // `$x eq "literal"` / `"literal" lt $y` / chained `$a eq "a" || $a eq "b"`
    // string comparisons now flow through the bridge via STK_VAL_LOAD_CONST.
    // Same borrow-only contract as concat — the compare helpers `sv_borrow` the
    // operands; nothing drains the LoadConst-produced bits.
    #[test]
    fn fusevm_runs_string_compare_with_load_const() {
        use Op::*;
        // `$x eq "yes"` shape: GetScalarSlot, LoadConst, StrEq.
        let seg_eq = vec![GetScalarSlot(0), LoadConst(0), StrEq];
        assert!(segment_is_string_compare_eligible(&seg_eq, 0));
        // `"a" lt $x` shape: LoadConst, GetScalarSlot, StrLt.
        let seg_lt = vec![LoadConst(0), GetScalarSlot(0), StrLt];
        assert!(segment_is_string_compare_eligible(&seg_lt, 0));

        let yes = StrykeValue::string("yes".to_string());
        let no = StrykeValue::string("no".to_string());
        let key = StrykeValue::string("yes".to_string());
        let constants = vec![key];

        for _ in 0..16 {
            let mut slots = [yes.raw_bits() as i64];
            let out = run_linear_segment(&seg_eq, 0, &mut slots, SubTerminator::Value, &constants)
                .expect("compare-with-literal must run on fusevm");
            assert_eq!(out.as_integer(), Some(1), "yes eq yes");

            let mut slots = [no.raw_bits() as i64];
            let out = run_linear_segment(&seg_eq, 0, &mut slots, SubTerminator::Value, &constants)
                .expect("compare-with-literal must run on fusevm");
            assert_eq!(out.as_integer(), Some(0), "no eq yes");
        }

        // "yes" lt "yes" = 0; "yes" lt "z" = 1 — exercise the LoadConst-on-left form.
        let z = StrykeValue::string("z".to_string());
        let constants_lit_left = vec![StrykeValue::string("yes".to_string())];
        let mut slots = [z.raw_bits() as i64];
        let out = run_linear_segment(&seg_lt, 0, &mut slots, SubTerminator::Value, &constants_lit_left)
            .expect("literal-on-left compare must run on fusevm");
        assert_eq!(out.as_integer(), Some(1), "yes lt z");
    }

    // `length($s) > 5` / `length($s) - 1` / `ord($c) >= 128` etc.: a unary
    // string→int builtin combined with an int literal via an integer binary op
    // (arithmetic or compare). Slot marshals as a string handle (consumed
    // immediately by the unary str builtin); the LoadInt + binop produce a
    // plain i64 result. Goes through the same str-family eager-block-JIT path
    // as bare `length($s)`.
    #[test]
    fn fusevm_runs_string_unary_int_combined() {
        use crate::bytecode::BuiltinId;
        // length($s) > 5
        let seg_gt = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Length as u16, 1),
            Op::LoadInt(5),
            Op::NumGt,
        ];
        assert!(segment_is_string_unary_int_combined_eligible(&seg_gt, 0));

        // length($s) - 1
        let seg_sub = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Length as u16, 1),
            Op::LoadInt(1),
            Op::Sub,
        ];
        assert!(segment_is_string_unary_int_combined_eligible(&seg_sub, 0));

        // ord($c) >= 128
        let seg_ord = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Ord as u16, 1),
            Op::LoadInt(128),
            Op::NumGe,
        ];
        assert!(segment_is_string_unary_int_combined_eligible(&seg_ord, 0));

        let hello_world = StrykeValue::string("hello world".to_string());
        let empty = StrykeValue::string("".to_string());
        let a = StrykeValue::string("A".to_string());

        for _ in 0..8 {
            let mut slots = [hello_world.raw_bits() as i64];
            let out = run_linear_segment(&seg_gt, 0, &mut slots, SubTerminator::Value, &[])
                .expect("length>N must run on fusevm");
            assert_eq!(out.as_integer(), Some(1), "length(\"hello world\") > 5");

            let mut slots = [empty.raw_bits() as i64];
            let out = run_linear_segment(&seg_gt, 0, &mut slots, SubTerminator::Value, &[])
                .expect("length>N must run on fusevm");
            assert_eq!(out.as_integer(), Some(0), "length(\"\") > 5");

            let mut slots = [hello_world.raw_bits() as i64];
            let out = run_linear_segment(&seg_sub, 0, &mut slots, SubTerminator::Value, &[])
                .expect("length-N must run on fusevm");
            assert_eq!(out.as_integer(), Some(10), "length(\"hello world\") - 1");

            let mut slots = [a.raw_bits() as i64];
            let out = run_linear_segment(&seg_ord, 0, &mut slots, SubTerminator::Value, &[])
                .expect("ord>=N must run on fusevm");
            assert_eq!(out.as_integer(), Some(0), "ord(\"A\") >= 128");
        }
    }

    // `length($a) > length($b)` / `length($a) - length($b)` / `ord($a) == ord($b)`:
    // two unary string→int builtins on two slots combined via an int binop.
    // Symmetric to the single-slot + int-literal variant: both slots marshal
    // as string handles (each consumed by its own unary str builtin), result
    // is a plain i64.
    #[test]
    fn fusevm_runs_string_unary_binop() {
        use crate::bytecode::BuiltinId;
        // length($a) > length($b)
        let seg_cmp = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Length as u16, 1),
            Op::GetScalarSlot(1),
            Op::CallBuiltin(BuiltinId::Length as u16, 1),
            Op::NumGt,
        ];
        assert!(segment_is_string_unary_binop_eligible(&seg_cmp, 0));

        // length($a) - length($b)
        let seg_sub = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Length as u16, 1),
            Op::GetScalarSlot(1),
            Op::CallBuiltin(BuiltinId::Length as u16, 1),
            Op::Sub,
        ];
        assert!(segment_is_string_unary_binop_eligible(&seg_sub, 0));

        let foo = StrykeValue::string("foo".to_string());
        let hello = StrykeValue::string("hello".to_string());
        let hi = StrykeValue::string("hi".to_string());

        for _ in 0..8 {
            // length("foo") > length("hello") = 3 > 5 = 0
            let mut slots = [foo.raw_bits() as i64, hello.raw_bits() as i64];
            let out = run_linear_segment(&seg_cmp, 0, &mut slots, SubTerminator::Value, &[])
                .expect("length-cmp-length must run on fusevm");
            assert_eq!(out.as_integer(), Some(0), "length(foo) > length(hello)");

            // length("hello") - length("hi") = 5 - 2 = 3
            let mut slots = [hello.raw_bits() as i64, hi.raw_bits() as i64];
            let out = run_linear_segment(&seg_sub, 0, &mut slots, SubTerminator::Value, &[])
                .expect("length-sub-length must run on fusevm");
            assert_eq!(out.as_integer(), Some(3), "length(hello) - length(hi)");
        }
    }

    // `"prefix" x $n` / `substr("abc", $n)` lower to fusevm: literal-string
    // operand resolved at runtime via STK_VAL_LOAD_CONST, slot operand
    // marshaled as a plain int (no slot is a string handle here — the literal
    // supplies the string). Result is an owned string handle.
    #[test]
    fn fusevm_runs_literal_string_repeat_and_substr() {
        use crate::bytecode::BuiltinId;
        // `"*" x $n` shape.
        let seg_rep = vec![Op::LoadConst(0), Op::GetScalarSlot(0), Op::StringRepeat];
        assert!(segment_is_literal_string_int_to_string_eligible(&seg_rep, 0));

        // `substr("Hello, World", $n)` shape.
        let seg_sub = vec![
            Op::LoadConst(0),
            Op::GetScalarSlot(0),
            Op::CallBuiltin(BuiltinId::Substr as u16, 2),
        ];
        assert!(segment_is_literal_string_int_to_string_eligible(&seg_sub, 0));

        // Repeat: "*" x 5 → "*****".
        let star = StrykeValue::string("*".to_string());
        let constants_star = vec![star];
        for _ in 0..16 {
            let mut slots = [5_i64];
            let out = run_linear_segment(
                &seg_rep,
                0,
                &mut slots,
                SubTerminator::Value,
                &constants_star,
            )
            .expect("literal-x-slot repeat must run on fusevm");
            assert_eq!(out.as_str().as_deref(), Some("*****"));
        }

        // Substr: substr("Hello, World", 7) → "World".
        let hello = StrykeValue::string("Hello, World".to_string());
        let constants_hello = vec![hello];
        for _ in 0..16 {
            let mut slots = [7_i64];
            let out = run_linear_segment(
                &seg_sub,
                0,
                &mut slots,
                SubTerminator::Value,
                &constants_hello,
            )
            .expect("literal-substr-slot must run on fusevm");
            assert_eq!(out.as_str().as_deref(), Some("World"));
        }
    }

    // `substr("Hello, World!", $off, $len)` — 3-arg substr with literal
    // string + two slot ints. Same dispatch family as the 2-arg case (`lit_str_int_ok`),
    // distinguished only by the additional GetScalarSlot for the length operand.
    #[test]
    fn fusevm_runs_literal_string_substr3() {
        use crate::bytecode::BuiltinId;
        let seg = vec![
            Op::LoadConst(0),
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::CallBuiltin(BuiltinId::Substr as u16, 3),
        ];
        assert!(segment_is_literal_string_int_to_string_eligible(&seg, 0));

        let hello = StrykeValue::string("Hello, World!".to_string());
        let constants = vec![hello];

        // (off, len, expected)
        let cases: &[(i64, i64, &str)] = &[
            (0, 5, "Hello"),
            (7, 5, "World"),
            (7, 6, "World!"),
            (0, 13, "Hello, World!"),
        ];
        for &(off, len, want) in cases {
            for _ in 0..8 {
                let mut slots = [off, len];
                let out = run_linear_segment(
                    &seg,
                    0,
                    &mut slots,
                    SubTerminator::Value,
                    &constants,
                )
                .expect("literal-substr-3 must run on fusevm");
                assert_eq!(out.as_str().as_deref(), Some(want), "substr(\"...\", {off}, {len})");
            }
        }
    }

    // `index($s, "needle")` / `index("haystack", $n)` lower to fusevm: same
    // (handle, handle) -> i64 ABI as the slot-slot form, with LoadConst supplying
    // one operand via STK_VAL_LOAD_CONST. Disk-cache safe by construction.
    #[test]
    fn fusevm_runs_string_index_with_load_const() {
        use crate::bytecode::BuiltinId;
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::LoadConst(0),
            Op::CallBuiltin(BuiltinId::Index as u16, 2),
        ];
        assert!(segment_is_string_binary_int_eligible(&seg, 0));

        let haystack = StrykeValue::string("foo needle bar".to_string());
        let needle = StrykeValue::string("needle".to_string());
        let constants = vec![needle];

        for _ in 0..16 {
            let mut slots = [haystack.raw_bits() as i64];
            let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &constants)
                .expect("index-with-literal must run on fusevm");
            assert_eq!(out.as_integer(), Some(4), "byte offset of \"needle\" in haystack");
        }
    }

    // The same chunk shape must produce a STABLE op_hash across runs with
    // different per-process constant pointers — the whole point of routing
    // LoadConst through STK_VAL_LOAD_CONST instead of baking the bits. Two
    // freshly-built `StrykeValue::string` constants have different `raw_bits`
    // each call (heap-allocated `Arc`), but the chunk hashes only the constant
    // *index* (0) and the helper id (FNV-1a of "stryke_val_load_const") —
    // both stable — so `op_hash` must match byte-for-byte.
    #[test]
    fn load_const_concat_chunk_op_hash_is_pointer_independent() {
        let seg = vec![Op::GetScalarSlot(0), Op::LoadConst(0), Op::Concat];

        let slot = StrykeValue::string("slot".to_string());
        let slots_buf = [slot.raw_bits() as i64];

        let lit1 = StrykeValue::string("first".to_string());
        let lit2 = StrykeValue::string("second".to_string());
        // Sanity: two freshly-built strings really do have distinct raw_bits.
        assert_ne!(
            lit1.raw_bits(),
            lit2.raw_bits(),
            "test assumes distinct per-process pointers"
        );

        let chunk_a = build_chunk(&seg, 0, &slots_buf, false, &[lit1])
            .expect("chunk a builds with LoadConst translation");
        let chunk_b = build_chunk(&seg, 0, &slots_buf, false, &[lit2])
            .expect("chunk b builds with LoadConst translation");

        assert_eq!(
            chunk_a.op_hash, chunk_b.op_hash,
            "LoadConst → STK_VAL_LOAD_CONST keeps op_hash independent of \
             per-process constant heap-pointer bits"
        );
    }

    // `length($s)` lowered to fusevm (unary `STK_STR_LEN` host helper) must equal
    // the interpreter's `StrykeValue::length_value` for ASCII, multibyte UTF-8 and
    // empty strings, under both the byte (default) and char (`use utf8`) pragmas.
    // The operand is only borrowed, so it must survive repeated runs intact.
    #[test]
    fn fusevm_runs_string_length() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(crate::bytecode::BuiltinId::Length as u16, 1),
        ];
        assert!(segment_is_string_unary_eligible(&seg, 0));

        let cases: &[&str] = &["", "foo", "hello world", "café", "naïve façade", "🦀🦀"];
        for utf8 in [false, true] {
            set_utf8_pragma(utf8);
            for s in cases {
                let v = StrykeValue::string((*s).to_string());
                let want = v.length_value(utf8);
                let mut last = None;
                for _ in 0..256 {
                    let mut slots = [v.raw_bits() as i64];
                    last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                        .expect("string-length segment must run on fusevm")
                        .as_integer();
                }
                assert_eq!(last, Some(want), "length({s:?}) utf8={utf8}");
                // The operand is borrowed, never dropped — it stays intact.
                assert_eq!(v.as_str().as_deref(), Some(*s));
            }
        }
        set_utf8_pragma(false);
    }

    // `ord`/`hex`/`oct` ($s) lowered to fusevm (unary host helpers) must equal the
    // interpreter's `StrykeValue::{ord,hex,oct}_value`. These are pragma-independent.
    #[test]
    fn fusevm_runs_string_ord_hex_oct() {
        use crate::bytecode::BuiltinId;
        let cases: &[(BuiltinId, &str, fn(&StrykeValue) -> i64)] = &[
            (BuiltinId::Ord, "A", |v| v.ord_value()),
            (BuiltinId::Ord, "abc", |v| v.ord_value()),
            (BuiltinId::Ord, "", |v| v.ord_value()),
            (BuiltinId::Ord, "🦀", |v| v.ord_value()),
            (BuiltinId::Hex, "ff", |v| v.hex_value()),
            (BuiltinId::Hex, "0x1A", |v| v.hex_value()),
            (BuiltinId::Hex, "deadbeef", |v| v.hex_value()),
            (BuiltinId::Hex, "zzz", |v| v.hex_value()),
            (BuiltinId::Oct, "0755", |v| v.oct_value()),
            (BuiltinId::Oct, "0x1A", |v| v.oct_value()),
            (BuiltinId::Oct, "0b1010", |v| v.oct_value()),
            (BuiltinId::Oct, "777", |v| v.oct_value()),
        ];
        for (builtin, s, want_fn) in cases {
            let seg = vec![
                Op::GetScalarSlot(0),
                Op::CallBuiltin(*builtin as u16, 1),
            ];
            assert!(
                segment_is_string_unary_eligible(&seg, 0),
                "segment must be unary-string eligible for {builtin:?}"
            );
            let v = StrykeValue::string((*s).to_string());
            let want = want_fn(&v);
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [v.raw_bits() as i64];
                last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("unary-string segment must run on fusevm")
                    .as_integer();
            }
            assert_eq!(last, Some(want), "{builtin:?}({s:?})");
            assert_eq!(v.as_str().as_deref(), Some(*s));
        }
    }

    // `uc`/`lc`/`ucfirst`/`lcfirst` ($s) lowered to fusevm (unary string→string host
    // helpers) must equal the interpreter's `StrykeValue::{uc,lc,ucfirst,lcfirst}_value`.
    // The result is a freshly allocated owned string handle (reconstructed via
    // `from_raw_bits`), and the borrowed operand must survive repeated runs intact.
    #[test]
    fn fusevm_runs_string_uc_lc() {
        use crate::bytecode::BuiltinId;
        let cases: &[(BuiltinId, &str, fn(&StrykeValue) -> String)] = &[
            (BuiltinId::Uc, "hello", |v| v.uc_value()),
            (BuiltinId::Uc, "MiXeD", |v| v.uc_value()),
            (BuiltinId::Uc, "", |v| v.uc_value()),
            (BuiltinId::Uc, "café", |v| v.uc_value()),
            (BuiltinId::Lc, "HELLO", |v| v.lc_value()),
            (BuiltinId::Lc, "MiXeD", |v| v.lc_value()),
            (BuiltinId::Lc, "ÉCOLE", |v| v.lc_value()),
            (BuiltinId::Ucfirst, "hello world", |v| v.ucfirst_value()),
            (BuiltinId::Ucfirst, "", |v| v.ucfirst_value()),
            (BuiltinId::Ucfirst, "éa", |v| v.ucfirst_value()),
            (BuiltinId::Lcfirst, "Hello World", |v| v.lcfirst_value()),
            (BuiltinId::Lcfirst, "ABC", |v| v.lcfirst_value()),
            (BuiltinId::Fc, "HELLO", |v| v.fc_value()),
            (BuiltinId::Fc, "Straße", |v| v.fc_value()),
            (BuiltinId::Fc, "MiXeD", |v| v.fc_value()),
            (BuiltinId::Fc, "", |v| v.fc_value()),
        ];
        for (builtin, s, want_fn) in cases {
            let seg = vec![
                Op::GetScalarSlot(0),
                Op::CallBuiltin(*builtin as u16, 1),
            ];
            assert!(
                segment_is_string_unary_eligible(&seg, 0),
                "segment must be unary-string eligible for {builtin:?}"
            );
            assert!(
                segment_is_string_unary_str_eligible(&seg, 0),
                "segment must be unary-string→string eligible for {builtin:?}"
            );
            let v = StrykeValue::string((*s).to_string());
            let want = want_fn(&v);
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [v.raw_bits() as i64];
                last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("unary-string→string segment must run on fusevm")
                    .as_str();
            }
            assert_eq!(last.as_deref(), Some(want.as_str()), "{builtin:?}({s:?})");
            // The operand is borrowed, never dropped — it stays intact.
            assert_eq!(v.as_str().as_deref(), Some(*s));
        }
    }

    // `chr($n)` lowered to fusevm (int→string `STK_STR_CHR` host helper) must equal
    // the interpreter's `StrykeValue::chr_value`. The operand is an *integer* (marshaled
    // unboxed), and the result is an owned string handle reconstructed via `from_raw_bits`.
    #[test]
    fn fusevm_runs_string_chr() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::CallBuiltin(crate::bytecode::BuiltinId::Chr as u16, 1),
        ];
        assert!(segment_is_int_to_string_eligible(&seg, 0));
        assert!(!segment_is_string_unary_eligible(&seg, 0));

        let cases: &[i64] = &[65, 97, 0x1F980, 8364, 0, 233, 0x110000];
        for n in cases {
            let want = crate::value::chr_from_codepoint(*n);
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [*n];
                last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("chr segment must run on fusevm")
                    .as_str();
            }
            assert_eq!(last.as_deref(), Some(want.as_str()), "chr({n})");
        }
    }

    // `index`/`rindex` ($s, $sub) (2-arg form) lowered to fusevm (binary string→int
    // host helpers) must equal the interpreter's `StrykeValue::{index,rindex}_value`.
    // Two string-handle operands, `i64` result; operands are only borrowed.
    #[test]
    fn fusevm_runs_string_index_rindex() {
        use crate::bytecode::BuiltinId;
        let cases: &[(BuiltinId, &str, &str)] = &[
            (BuiltinId::Index, "hello world", "o"),
            (BuiltinId::Index, "hello world", "world"),
            (BuiltinId::Index, "hello", "z"),
            (BuiltinId::Index, "abc", ""),
            (BuiltinId::Index, "café au lait", "au"),
            (BuiltinId::Rindex, "hello world", "o"),
            (BuiltinId::Rindex, "hello world", "l"),
            (BuiltinId::Rindex, "hello", "z"),
            (BuiltinId::Rindex, "abcabc", "bc"),
        ];
        for (builtin, s, sub) in cases {
            let seg = vec![
                Op::GetScalarSlot(0),
                Op::GetScalarSlot(1),
                Op::CallBuiltin(*builtin as u16, 2),
            ];
            assert!(
                segment_is_string_binary_int_eligible(&seg, 0),
                "segment must be binary-string→int eligible for {builtin:?}"
            );
            let a = StrykeValue::string((*s).to_string());
            let b = StrykeValue::string((*sub).to_string());
            let want = if *builtin == BuiltinId::Index {
                a.index_value(&b)
            } else {
                a.rindex_value(&b)
            };
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [a.raw_bits() as i64, b.raw_bits() as i64];
                last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("binary-string→int segment must run on fusevm")
                    .as_integer();
            }
            assert_eq!(last, Some(want), "{builtin:?}({s:?}, {sub:?})");
            // Operands are only borrowed by the helper, so they remain intact.
            assert_eq!(a.as_str().as_deref(), Some(*s));
            assert_eq!(b.as_str().as_deref(), Some(*sub));
        }
    }

    // `substr($s,$off)` (2-arg) and `$s x $n` lowered to fusevm (binary
    // string+integer→string host helpers) must equal the interpreter's
    // `StrykeValue::{substr2,repeat}_value`. Operand 0 is a string handle (raw bits),
    // operand 1 a plain integer; the result is an owned string handle.
    #[test]
    fn fusevm_runs_string_substr_repeat() {
        enum Kind {
            Substr,
            Repeat,
        }
        let cases: &[(Kind, u16, &str, i64)] = &[
            (Kind::Substr, crate::bytecode::BuiltinId::Substr as u16, "hello world", 0),
            (Kind::Substr, crate::bytecode::BuiltinId::Substr as u16, "hello world", 6),
            (Kind::Substr, crate::bytecode::BuiltinId::Substr as u16, "hello world", -3),
            (Kind::Substr, crate::bytecode::BuiltinId::Substr as u16, "hello world", 20),
            (Kind::Substr, crate::bytecode::BuiltinId::Substr as u16, "café", 2),
            (Kind::Repeat, 0, "ab", 0),
            (Kind::Repeat, 0, "ab", 1),
            (Kind::Repeat, 0, "ab", 4),
            (Kind::Repeat, 0, "xy", -2),
        ];
        for (kind, builtin, s, n) in cases {
            let op = match kind {
                Kind::Substr => Op::CallBuiltin(*builtin, 2),
                Kind::Repeat => Op::StringRepeat,
            };
            let seg = vec![Op::GetScalarSlot(0), Op::GetScalarSlot(1), op];
            assert!(
                segment_is_string_int_to_string_eligible(&seg, 0),
                "segment must be binary-string+int→string eligible for {s:?} x/substr {n}"
            );
            assert_eq!(string_handle_slot(&seg, 0), Some(0));
            let a = StrykeValue::string((*s).to_string());
            let want = match kind {
                Kind::Substr => a.substr2_value(*n),
                Kind::Repeat => a.repeat_value(*n),
            };
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [a.raw_bits() as i64, *n];
                last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("binary-string+int→string segment must run on fusevm")
                    .as_str();
            }
            assert_eq!(last.as_deref(), Some(want.as_str()), "{s:?} op {n}");
            // The string operand is only borrowed, so it remains intact.
            assert_eq!(a.as_str().as_deref(), Some(*s));
        }
    }

    // 3-arg `substr($s,$off,$len)` lowered to fusevm (ternary string+int+int→string
    // host helper) must equal the interpreter's `StrykeValue::substr3_value`. Operand 0
    // is a string handle, operands 1/2 plain integers; the result is an owned handle.
    #[test]
    fn fusevm_runs_string_substr3() {
        let s = "hello world";
        let cases: &[(i64, i64)] = &[
            (0, 5),
            (6, 5),
            (6, 100),
            (-5, 3),
            (3, -2),
            (0, -3),
            (20, 4),
            (4, 0),
        ];
        for (off, len) in cases {
            let seg = vec![
                Op::GetScalarSlot(0),
                Op::GetScalarSlot(1),
                Op::GetScalarSlot(2),
                Op::CallBuiltin(crate::bytecode::BuiltinId::Substr as u16, 3),
            ];
            assert!(
                segment_is_string_int2_to_string_eligible(&seg, 0),
                "segment must be ternary-string+int+int→string eligible for substr({off},{len})"
            );
            assert!(segment_is_string_int_mixed_eligible(&seg, 0));
            assert_eq!(string_handle_slot(&seg, 0), Some(0));
            let a = StrykeValue::string(s.to_string());
            let want = a.substr3_value(*off, *len);
            let mut last = None;
            for _ in 0..256 {
                let mut slots = [a.raw_bits() as i64, *off, *len];
                last = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                    .expect("ternary-string+int+int→string segment must run on fusevm")
                    .as_str();
            }
            assert_eq!(last.as_deref(), Some(want.as_str()), "substr({s:?},{off},{len})");
            assert_eq!(a.as_str().as_deref(), Some(s));
        }
    }

    // The concat chunk, like the compare chunks, must not bake operand pointers
    // into its op_hash: it persists exactly one disk-cache blob that is reused
    // across different operand strings, and the result is still correct.
    #[test]
    fn string_concat_segment_produces_stable_disk_cache_entry() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Concat,
        ];

        let dir = std::env::temp_dir().join(format!(
            "stryke_concatcache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FUSEVM_JIT_CACHE_DIR", &dir);

        let prefix = {
            let p1 = StrykeValue::string("p1".to_string());
            let p2 = StrykeValue::string("p2".to_string());
            let slots = [p1.raw_bits() as i64, p2.raw_bits() as i64];
            let chunk = build_chunk(&seg, 0, &slots, false, &[]).expect("probe chunk");
            format!("{:016x}.", chunk.op_hash)
        };
        let count_blobs = || -> usize {
            std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".blk.fjit")
                })
                .count()
        };

        let a1 = StrykeValue::string("alpha".to_string());
        let b1 = StrykeValue::string("-one".to_string());
        let mut slots1 = [a1.raw_bits() as i64, b1.raw_bits() as i64];
        let out1 = run_linear_segment(&seg, 0, &mut slots1, SubTerminator::Value, &[]);
        assert_eq!(out1.and_then(|v| v.as_str()).as_deref(), Some("alpha-one"));
        let after_first = count_blobs();

        let a2 = StrykeValue::string("gamma-distinct".to_string());
        let b2 = StrykeValue::string("-two-distinct".to_string());
        let mut slots2 = [a2.raw_bits() as i64, b2.raw_bits() as i64];
        let out2 = run_linear_segment(&seg, 0, &mut slots2, SubTerminator::Value, &[]);
        assert_eq!(
            out2.and_then(|v| v.as_str()).as_deref(),
            Some("gamma-distinct-two-distinct")
        );
        let after_second = count_blobs();

        std::env::remove_var("FUSEVM_JIT_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(after_first, 1, "concat segment must persist exactly one blob");
        assert_eq!(
            after_second, after_first,
            "different operands must reuse the same concat cache blob"
        );
    }

    // A pure-integer sub body seeded from `@_` (e.g. `sub addnums { my ($a,$b)=@_;
    // return $a+$b }`) must also persist exactly ONE native blob, no matter how many
    // DISTINCT argument values it is called with. The integer block-JIT path builds
    // an *unseeded* chunk so the marshaled args flow through `slot_buf` (via `GetSlot`)
    // instead of being baked in as `LoadInt` immediates; without this fix a hot sub
    // called with thousands of distinct args would spill one cache file per arg combo.
    #[test]
    fn integer_arg_segment_produces_stable_disk_cache_entry() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Add,
        ];
        assert!(segment_is_fusevm_eligible(&seg, 0));

        let dir = std::env::temp_dir().join(format!(
            "stryke_intargcache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FUSEVM_JIT_CACHE_DIR", &dir);

        let prefix = {
            let slots = [3_i64, 4_i64];
            let chunk = build_chunk(&seg, 0, &slots, false, &[]).expect("probe chunk");
            format!("{:016x}.", chunk.op_hash)
        };
        let count_blobs = || -> usize {
            std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".blk.fjit")
                })
                .count()
        };

        let mut slots1 = [100_i64, 23_i64];
        let out1 = run_linear_segment(&seg, 0, &mut slots1, SubTerminator::Value, &[]);
        assert_eq!(out1.and_then(|v| v.as_integer()), Some(123));
        let after_first = count_blobs();

        // Thousands of distinct argument pairs must all reuse the same blob.
        for i in 0..2000_i64 {
            let mut slots = [i * 7 + 1, i * 13 + 2];
            let got = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
                .and_then(|v| v.as_integer());
            assert_eq!(got, Some((i * 7 + 1) + (i * 13 + 2)));
        }
        let after_many = count_blobs();

        std::env::remove_var("FUSEVM_JIT_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(after_first, 1, "integer-arg segment must persist exactly one blob");
        assert_eq!(
            after_many, after_first,
            "distinct argument values must reuse the same cache blob (no per-arg explosion)"
        );
    }

    // End-to-end: running a string-compare segment with an on-disk cache dir set
    // and re-running with DIFFERENT operand strings reuses that same blob (no new
    // file, no unbounded growth). This is the concrete proof that the safety fix
    // makes string compares genuinely disk-cacheable.
    #[test]
    fn string_compare_segment_produces_stable_disk_cache_entry() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::StrLt,
        ];

        // Unique temp cache dir so we only ever count OUR op_hash's blobs (other
        // concurrently-running tests may also write here once the env var is set,
        // but they use different op_hashes, so filtering by prefix stays race-safe).
        let dir = std::env::temp_dir().join(format!(
            "stryke_strcache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FUSEVM_JIT_CACHE_DIR", &dir);

        let prefix = {
            let probe = StrykeValue::string("probe-a".to_string());
            let probe2 = StrykeValue::string("probe-b".to_string());
            let slots = [probe.raw_bits() as i64, probe2.raw_bits() as i64];
            let chunk = build_chunk(&seg, 0, &slots, false, &[]).expect("probe chunk");
            format!("{:016x}.", chunk.op_hash)
        };
        let count_blobs = || -> usize {
            std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name();
                    let n = n.to_string_lossy();
                    n.starts_with(&prefix) && n.ends_with(".blk.fjit")
                })
                .count()
        };

        // Run 1.
        let a1 = StrykeValue::string("apple".to_string());
        let b1 = StrykeValue::string("banana".to_string());
        let mut slots1 = [a1.raw_bits() as i64, b1.raw_bits() as i64];
        let out1 = run_linear_segment(&seg, 0, &mut slots1, SubTerminator::Value, &[]);
        assert_eq!(out1.and_then(|v| v.as_integer()), Some(1), "apple < banana");
        let after_first = count_blobs();

        // Run 2 with DIFFERENT operand strings (different pointers).
        let a2 = StrykeValue::string("cherry-distinct".to_string());
        let b2 = StrykeValue::string("date-distinct".to_string());
        let mut slots2 = [a2.raw_bits() as i64, b2.raw_bits() as i64];
        let out2 = run_linear_segment(&seg, 0, &mut slots2, SubTerminator::Value, &[]);
        assert_eq!(out2.and_then(|v| v.as_integer()), Some(1), "cherry < date");
        let after_second = count_blobs();

        std::env::remove_var("FUSEVM_JIT_CACHE_DIR");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            after_first, 1,
            "string-compare segment must persist exactly one native blob"
        );
        assert_eq!(
            after_second, after_first,
            "different operand strings must REUSE the same cache blob (pointer-independent key)"
        );
    }

    // A string-compare chunk must NOT bake operand handles into its op_hash (the
    // disk-cache key): different live string pointers must yield byte-identical
    // chunk ops and therefore the SAME op_hash, so the on-disk native blob is
    // reused across runs/values instead of growing unbounded — and so no stale
    // pointer is ever frozen into cached native code.
    #[test]
    fn string_compare_chunk_op_hash_is_pointer_independent() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::StrEq,
        ];
        // Two completely different operand sets → different raw pointer handles.
        let a1 = StrykeValue::string("alpha".to_string());
        let b1 = StrykeValue::string("beta".to_string());
        let a2 = StrykeValue::string("gamma-distinct".to_string());
        let b2 = StrykeValue::string("delta-distinct".to_string());
        let slots1 = [a1.raw_bits() as i64, b1.raw_bits() as i64];
        let slots2 = [a2.raw_bits() as i64, b2.raw_bits() as i64];
        assert_ne!(slots1, slots2, "operand handles must actually differ");

        let chunk1 = build_chunk(&seg, 0, &slots1, false, &[]).expect("chunk 1");
        let chunk2 = build_chunk(&seg, 0, &slots2, false, &[]).expect("chunk 2");
        assert_eq!(
            chunk1.op_hash, chunk2.op_hash,
            "unseeded string-compare chunks must hash identically regardless of operand pointers"
        );

        // Sanity: the SEEDED build (numeric path) WOULD bake the values and thus
        // differ — this is exactly why string segments must use the unseeded form.
        let seeded1 = build_chunk(&seg, 0, &slots1, true, &[]).expect("seeded 1");
        let seeded2 = build_chunk(&seg, 0, &slots2, true, &[]).expect("seeded 2");
        assert_ne!(
            seeded1.op_hash, seeded2.op_hash,
            "seeded chunks bake operand values, so their hashes diverge"
        );
    }

    // The shared comparator matches strykelang's native byte-level semantics and
    // never drops its borrowed operands (Arc strong count is preserved).
    #[test]
    fn string_compare_op_preserves_operands() {
        let a = StrykeValue::string("hello".to_string());
        let b = StrykeValue::string("hello".to_string());
        for _ in 0..1000 {
            assert_eq!(
                stryke_str_compare_op(ext_ops::STK_STR_EQ, a.raw_bits() as i64, b.raw_bits() as i64),
                1
            );
        }
        // If the helper had dropped a borrow, this would have use-after-freed above.
        assert!(a.str_eq(&b));
    }

    // The `floored_mod` reference helper matches Perl `%` across signs.
    #[test]
    fn floored_mod_reference_semantics() {
        assert_eq!(floored_mod(-7, 3), 2);
        assert_eq!(floored_mod(7, -3), -2);
        assert_eq!(floored_mod(7, 3), 1);
        assert_eq!(floored_mod(-7, -3), -1);
        assert_eq!(floored_mod(i64::MIN, -1), 0); // guarded
        assert_eq!(floored_mod(5, 0), 0); // guarded
    }

    // Pure arithmetic with no slots: (40 + 2) * 3 - 6 == 120, executed on fusevm.
    #[test]
    fn fusevm_runs_translated_arithmetic() {
        let seg = vec![
            Op::LoadInt(40),
            Op::LoadInt(2),
            Op::Add,
            Op::LoadInt(3),
            Op::Mul,
            Op::LoadInt(6),
            Op::Sub,
        ];
        let mut slots: [i64; 0] = [];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("segment must be fusevm-eligible and run");
        assert_eq!(out.as_integer(), Some(120));
    }

    // Slot round-trip: read slot 0 (=7), double it, write back; return value = 14.
    #[test]
    fn fusevm_reads_and_writes_slots() {
        let seg = vec![
            Op::GetScalarSlot(0),
            Op::LoadInt(2),
            Op::Mul,
            Op::SetScalarSlot(0),
            Op::GetScalarSlot(0),
        ];
        let mut slots = [7_i64];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("slot segment must run on fusevm");
        assert_eq!(slots[0], 14);
        assert_eq!(out.as_integer(), Some(14));
    }

    // In-segment forward jump remaps by the preamble offset and still lands right.
    #[test]
    fn fusevm_remaps_in_segment_jump() {
        // seg_start = 100; Jump(103) skips the LoadInt(999) at rel idx 1.
        let seg = vec![
            Op::Jump(103), // abs 100 -> skip to abs 103
            Op::LoadInt(999),
            Op::LoadInt(888),
            Op::LoadInt(5), // abs 103: landing pad
        ];
        let mut slots: [i64; 0] = [];
        let out = run_linear_segment(&seg, 100, &mut slots, SubTerminator::Value, &[])
            .expect("jump segment must run on fusevm");
        assert_eq!(out.as_integer(), Some(5));
    }

    // Pre-increment a slot and stack Inc/Dec all lower onto fusevm: slot 0 = 10,
    // `++$slot` → 11 (written back), then push it, Inc → 12, Dec → 11.
    #[test]
    fn fusevm_runs_slot_preinc_and_stack_incdec() {
        let seg = vec![
            Op::PreIncSlot(0), // slot 0: 10 -> 11, pushes 11
            Op::Inc,           // 11 -> 12
            Op::Dec,           // 12 -> 11
        ];
        let mut slots = [10_i64];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("preinc/inc/dec segment must run on fusevm");
        assert_eq!(slots[0], 11);
        assert_eq!(out.as_integer(), Some(11));
    }

    // A non-universal op (string concat) makes the segment ineligible: caller
    // falls back, signaled by None.
    #[test]
    fn ineligible_segment_returns_none() {
        let seg = vec![Op::LoadInt(1), Op::Concat];
        let mut slots: [i64; 0] = [];
        assert!(run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[]).is_none());
    }

    // Void terminator is not handled by the universal tier.
    #[test]
    fn void_terminator_returns_none() {
        let seg = vec![Op::LoadInt(1)];
        let mut slots: [i64; 0] = [];
        assert!(run_linear_segment(&seg, 0, &mut slots, SubTerminator::Void, &[]).is_none());
    }

    // strykelang's fused slot-arith-assign ops decompose onto fusevm primitives.
    // `my $a=7; my $b=3; my $c=$a+$b; $c+=$a; $c-=$b; return $c` == 14.
    #[test]
    fn fusevm_runs_fused_slot_assign_ops() {
        let seg = vec![
            Op::LoadInt(7),
            Op::DeclareScalarSlot(0, 0), // $a = 7
            Op::LoadInt(3),
            Op::DeclareScalarSlot(1, 0), // $b = 3
            Op::GetScalarSlot(0),
            Op::GetScalarSlot(1),
            Op::Add,
            Op::DeclareScalarSlot(2, 0),      // $c = $a + $b = 10
            Op::AddAssignSlotSlotVoid(2, 0),  // $c += $a -> 17 (void)
            Op::SubAssignSlotSlot(2, 1),      // $c -= $b -> 14 (pushes 14)
            Op::Pop,
            Op::GetScalarSlot(2), // push $c = 14
        ];
        let mut slots = [0_i64; 3];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("fused slot-assign segment must run on fusevm");
        assert_eq!(slots[2], 14);
        assert_eq!(out.as_integer(), Some(14));
    }

    // `MulAssignSlotSlot` push-variant: $x=6; $y=7; $x*=$y == 42, left on stack.
    #[test]
    fn fusevm_runs_mul_assign_slot() {
        let seg = vec![
            Op::LoadInt(6),
            Op::DeclareScalarSlot(0, 0), // $x = 6
            Op::LoadInt(7),
            Op::DeclareScalarSlot(1, 0),  // $y = 7
            Op::MulAssignSlotSlot(0, 1),  // $x *= $y -> 42 (pushes 42)
        ];
        let mut slots = [0_i64; 2];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("mul-assign-slot segment must run on fusevm");
        assert_eq!(slots[0], 42);
        assert_eq!(out.as_integer(), Some(42));
    }

    // A counted loop built from the generic fused loop ops lowers to fusevm with
    // its in-segment jump targets remapped: sum of 0..4 == 10.
    #[test]
    fn fusevm_runs_counted_loop() {
        // idx: 0..=7 (seg_start = 0)
        let seg = vec![
            Op::LoadInt(0),
            Op::DeclareScalarSlot(0, 0), // 1: $sum = 0  (slot 0)
            Op::LoadInt(0),
            Op::DeclareScalarSlot(1, 0),        // 3: $i = 0    (slot 1)
            Op::SlotLtIntJumpIfFalse(1, 5, 7),  // 4: if !($i<5) goto 7 (exit)
            Op::AddAssignSlotSlotVoid(0, 1),    // 5: body: $sum += $i
            Op::SlotIncLtIntJumpBack(1, 5, 5),  // 6: $i++; if $i<5 goto 5
            Op::GetScalarSlot(0),               // 7: exit: push $sum
        ];
        let mut slots = [0_i64; 2];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("counted-loop segment must run on fusevm");
        assert_eq!(out.as_integer(), Some(10));
    }

    // The fully-fused `AccumSumLoop` lowers 1:1: sum of 0..4 == 10.
    #[test]
    fn fusevm_runs_accum_sum_loop() {
        let seg = vec![
            Op::LoadInt(0),
            Op::DeclareScalarSlot(0, 0), // $sum = 0
            Op::LoadInt(0),
            Op::DeclareScalarSlot(1, 0), // $i = 0
            Op::AccumSumLoop(0, 1, 5),   // while $i<5 { $sum += $i; $i += 1 }
            Op::GetScalarSlot(0),        // push $sum
        ];
        let mut slots = [0_i64; 2];
        let out = run_linear_segment(&seg, 0, &mut slots, SubTerminator::Value, &[])
            .expect("accum-sum-loop segment must run on fusevm");
        assert_eq!(out.as_integer(), Some(10));
    }
}
