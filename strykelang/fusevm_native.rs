//! Phase 1 of retiring strykelang's own VM (`crate::vm`) and JIT (`crate::jit`)
//! in favor of running *whole programs* on the shared [`fusevm`] runtime using
//! **native fusevm Values** (not the i64 bit-handles the numeric-segment bridge
//! in [`crate::fusevm_bridge`] uses).
//!
//! Today [`crate::fusevm_bridge`] offloads only eligible numeric segments to
//! fusevm; everything else runs on `crate::vm`. The end state is that the entire
//! program lowers to a fusevm `Chunk` (universal ops + Extended ops backed by a
//! stryke host) and runs on `fusevm::VM`, after which `vm.rs`/`jit.rs` are
//! deleted and stryke becomes a true fusevm frontend (so `--aot` works exactly
//! like zshrs/vimlrs/awkrs).
//!
//! This module is that path, built incrementally. It is opt-in via the
//! `STRYKE_FUSEVM_ONLY` environment variable and currently covers the
//! integer/float arithmetic subset, returning the program's value. Crucially,
//! [`try_run_native`] **aborts to `None` on any op outside the covered subset**,
//! so the caller falls back to `crate::vm` — the migration never silently
//! diverges; coverage only ever grows behind passing parity checks.

use crate::bytecode::{Chunk, Op};
use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;
use crate::vm_helper::VMHelper;
use indexmap::IndexMap;
use std::sync::Arc;

/// strykelang Extended-op IDs handled by [`native_ext_handler`] on the
/// fusevm-only path. These operate on **native fusevm Values** (distinct from
/// the i64-handle Extended ops in [`crate::fusevm_bridge`]).
mod nops {
    /// Perl `.` string concatenation (pops 2, pushes the concatenated string).
    pub const CONCAT: u16 = 0;
    /// String comparisons (`eq ne lt gt le ge`); each pops 2, pushes Int 1/0.
    pub const STR_EQ: u16 = 1;
    pub const STR_NE: u16 = 2;
    pub const STR_LT: u16 = 3;
    pub const STR_GT: u16 = 4;
    pub const STR_LE: u16 = 5;
    pub const STR_GE: u16 = 6;
    /// Numeric comparisons (`== != < > <= >=`); each pops 2, pushes Int 1/0.
    pub const NUM_EQ: u16 = 7;
    pub const NUM_NE: u16 = 8;
    pub const NUM_LT: u16 = 9;
    pub const NUM_GT: u16 = 10;
    pub const NUM_LE: u16 = 11;
    pub const NUM_GE: u16 = 12;
    /// `<=>` numeric spaceship (pops 2, pushes Int -1/0/1).
    pub const SPACESHIP: u16 = 13;
    /// Perl `!` logical-not (pops 1, pushes Int 1/0).
    pub const LOG_NOT: u16 = 14;
    /// Normalize top-of-stack to Perl truthiness as Int 1/0, so fusevm's
    /// conditional jumps branch using strykelang's `is_true` (fusevm's native
    /// truthiness differs for values like the string "0").
    pub const TRUTHY: u16 = 15;
    /// Arithmetic that can fault or change type (`/ % **`); each pops 2, pushes
    /// the result (or records a runtime error via the error slot).
    pub const DIV: u16 = 16;
    pub const MOD: u16 = 17;
    pub const POW: u16 = 18;
    /// Name-scoped scalars (globals / package / special vars). Each is preceded
    /// by a `LoadInt(name_idx)` so the handler can resolve the name; they
    /// delegate to the interp via the host below.
    pub const GET_SCALAR: u16 = 19;
    pub const SET_SCALAR: u16 = 20;
    pub const DECLARE_SCALAR: u16 = 21;
    /// "Plain" scalar access — direct `scope.get_scalar`/`set_scalar` (no
    /// special-var resolution); what the compiler emits for ordinary names.
    pub const GET_SCALAR_PLAIN: u16 = 22;
    pub const SET_SCALAR_PLAIN: u16 = 23;
    pub const SET_SCALAR_KEEP_PLAIN: u16 = 24;
    /// Arrays. MAKE_ARRAY pops a count then that many values (flattening nested
    /// arrays, Perl list semantics) into a fusevm `Value::Array`. DECLARE_ARRAY
    /// stores it in the interp scope by name. GET_ARRAY_ELEM reads `name[index]`
    /// with strykelang's indexing sugar. All preceded by their `LoadInt` args.
    pub const MAKE_ARRAY: u16 = 25;
    pub const DECLARE_ARRAY: u16 = 26;
    pub const GET_ARRAY_ELEM: u16 = 27;
    /// Hashes. DECLARE_HASH folds a flat k/v list (built via MAKE_ARRAY) into a
    /// map in the interp scope; GET_HASH_ELEM reads `name{key}`.
    pub const DECLARE_HASH: u16 = 28;
    pub const GET_HASH_ELEM: u16 = 29;
    /// I/O: `print` / `say` to the default handle. The Extended `arg` carries
    /// the argument count.
    pub const PRINT: u16 = 30;
    pub const SAY: u16 = 31;
    /// Static user-sub call. Preceded by `LoadInt(name_idx), LoadInt(argc),
    /// LoadInt(wantarray)`; delegates the whole call (scopes, param binding,
    /// recursion) to the interp via `call_named_sub`.
    pub const CALL_SUB: u16 = 32;
    /// Closures. MAKE_CODEREF (preceded by `LoadInt(block_idx), LoadInt(sig_idx)`)
    /// builds an anon sub capturing the current scope → registry handle.
    /// ARROW_CALL (`$f->(args)`) calls the coderef; `arg` carries wantarray.
    pub const MAKE_CODEREF: u16 = 33;
    pub const ARROW_CALL: u16 = 34;
    /// Scalar slots backed by `interp.scope` (not fusevm frame slots), so that
    /// closures capturing `my` locals see them via `scope.capture()`. Each is
    /// preceded by `LoadInt(slot)` (and `LoadInt(name_idx)` for DECLARE).
    pub const SLOT_GET: u16 = 35;
    pub const SLOT_SET: u16 = 36;
    pub const SLOT_SET_KEEP: u16 = 37;
    pub const SLOT_DECLARE: u16 = 38;
    /// `=~` match. Preceded by `LoadInt(pat_idx), LoadInt(flags_idx),
    /// LoadInt(scalar_g), LoadInt(pos_key_idx)`; delegates to
    /// `interp.regex_match_execute` (which also sets `$1`/`$&`/etc.).
    pub const REGEX_MATCH: u16 = 39;
    /// `s///` substitution. Preceded by `LoadInt(pat), LoadInt(repl),
    /// LoadInt(flags), LoadInt(lvalue_idx)`; delegates to
    /// `interp.regex_subst_execute`, which writes the result back to the lvalue
    /// and returns the substitution count.
    pub const REGEX_SUBST: u16 = 40;
    /// Perl `++` on a slot via `perl_inc` (magic string/number increment).
    /// INC_SLOT_VOID is `++slot` (result discarded); POST_INC_SLOT pushes the
    /// old value then increments (`slot++`). Preceded by `LoadInt(slot)`.
    pub const INC_SLOT_VOID: u16 = 41;
    pub const POST_INC_SLOT: u16 = 42;
}

/// `print`/`say` to the default handle, delegated to the interp so output goes
/// through the exact same sink (`write_formatted_print`), stringification
/// (`stringify_value`), and `$,`/`$\` (ofs/ors) that vm.rs uses → identical
/// output. `say` appends a newline and requires the `say` feature.
fn do_print(vm: &mut fusevm::VM, argc: usize, say: bool) {
    let mut args: Vec<StrykeValue> = Vec::with_capacity(argc);
    for _ in 0..argc {
        args.push(pop_stryke(vm));
    }
    args.reverse(); // pops are last-to-first → restore source order
    let r: Result<(), StrykeError> = with_interp(|i| {
        if say && (i.feature_bits & crate::vm_helper::FEAT_SAY) == 0 {
            return Err(StrykeError::runtime(
                "say() is disabled (enable with use feature 'say' or use feature ':5.10')",
                0,
            ));
        }
        let ofs = i.ofs.clone();
        let ors = i.ors.clone();
        let stringify = |i: &mut VMHelper, v: StrykeValue| -> Result<String, StrykeError> {
            match i.stringify_value(v, 0) {
                Ok(s) => Ok(s),
                Err(crate::vm_helper::FlowOrError::Error(e)) => Err(e),
                Err(_) => Err(StrykeError::runtime("print: unexpected control flow", 0)),
            }
        };
        let mut output = String::new();
        if args.is_empty() {
            let topic = i.scope.get_scalar("_");
            output.push_str(&stringify(i, topic)?);
        } else {
            for (idx, arg) in args.iter().enumerate() {
                if idx > 0 && !ofs.is_empty() {
                    output.push_str(&ofs);
                }
                for item in arg.to_list() {
                    output.push_str(&stringify(i, item)?);
                }
            }
        }
        if say {
            output.push('\n');
        }
        output.push_str(&ors);
        let dph = i.default_print_handle.clone();
        let handle = i.resolve_io_handle_name(&dph);
        i.write_formatted_print(&handle, &output, 0)
    });
    if let Err(e) = r {
        set_native_err(e);
    }
    // `print`/`say` are void in strykelang (leave nothing on the stack).
}

/// Index a string by Unicode char (negative-from-end), as strykelang's array
/// sugar does. Out of range → UNDEF.
fn char_index(s: &str, index: i64) -> StrykeValue {
    let cnt = s.chars().count() as i64;
    let i = if index < 0 { index + cnt } else { index };
    if i >= 0 && i < cnt {
        s.chars()
            .nth(i as usize)
            .map(|c| StrykeValue::string(c.to_string()))
            .unwrap_or(StrykeValue::UNDEF)
    } else {
        StrykeValue::UNDEF
    }
}

/// `name[index]` with strykelang's sugar — mirrors vm.rs's GetArrayElem exactly
/// (same scope methods), so behavior can't diverge. (Temporary twin of the vm.rs
/// arm; removed when vm.rs is deleted at the end of the migration.)
fn array_elem_value(i: &mut VMHelper, n: &str, index: i64) -> StrykeValue {
    if let Some(real) = n.strip_prefix("__topicstr__") {
        let s = i.scope.get_scalar(real).to_string();
        return char_index(&s, index);
    }
    if !crate::compat_mode() && i.scope.scalar_binding_exists(n) && i.scope.get_array(n).is_empty() {
        let s = i.scope.get_scalar(n).to_string();
        if !s.is_empty() {
            return char_index(&s, index);
        }
    }
    i.scope.get_array_element(n, index)
}

// ── Interp host ─────────────────────────────────────────────────────────────
// The structural keystone for the heap/call/name-scoped half of the migration:
// a thread-local view of the live `VMHelper` + the chunk's name table, set for
// the duration of the native run. Handlers reach interp state (scopes, special
// vars, overloads, …) through `with_interp`, mirroring awkrs's `CURRENT_RT` and
// zshrs's `CURRENT_EXECUTOR`. The guard brackets `vm.run()` exactly (the run is
// synchronous), so no aliasing or leak occurs.
thread_local! {
    static CURRENT_INTERP: std::cell::Cell<*mut VMHelper> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
    static CURRENT_CHUNK: std::cell::Cell<*const Chunk> =
        const { std::cell::Cell::new(std::ptr::null()) };
    /// Registry of non-scalar StrykeValues (closures, refs, regexes, objects)
    /// that fusevm's native `Value` can't hold. They ride the fusevm stack as
    /// `Value::NativeFn(id)` indices into this per-run table.
    static REGISTRY: std::cell::RefCell<Vec<StrykeValue>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct HostGuard;
impl HostGuard {
    fn enter(interp: &mut VMHelper, chunk: &Chunk) -> Self {
        CURRENT_INTERP.with(|c| c.set(interp as *mut VMHelper));
        CURRENT_CHUNK.with(|c| c.set(chunk as *const Chunk));
        HostGuard
    }
}
impl Drop for HostGuard {
    fn drop(&mut self) {
        CURRENT_INTERP.with(|c| c.set(std::ptr::null_mut()));
        CURRENT_CHUNK.with(|c| c.set(std::ptr::null()));
    }
}

/// Run `f` against the live interp. The host guard is always active while the
/// native VM runs, so the pointer is non-null inside any handler.
fn with_interp<R>(f: impl FnOnce(&mut VMHelper) -> R) -> R {
    let p = CURRENT_INTERP.with(|c| c.get());
    debug_assert!(!p.is_null(), "with_interp outside a HostGuard scope");
    // SAFETY: set from a live `&mut VMHelper` by `HostGuard::enter`, cleared on
    // drop; the guard brackets the synchronous run so no aliasing occurs.
    f(unsafe { &mut *p })
}

/// Run `f` against the live chunk (for names / blocks / code_ref_sigs).
fn with_chunk<R>(f: impl FnOnce(&Chunk) -> R) -> R {
    let p = CURRENT_CHUNK.with(|c| c.get());
    debug_assert!(!p.is_null(), "with_chunk outside a HostGuard scope");
    // SAFETY: set to the live chunk for the run's duration.
    f(unsafe { &*p })
}

/// Resolve a name-pool index to its name (chunk.names), via the host.
fn host_name(idx: i64) -> String {
    with_chunk(|c| c.names.get(idx as usize).cloned().unwrap_or_default())
}

/// The variant name of an op (Debug, without operands), for coverage tracing.
fn op_name(op: &Op) -> String {
    let s = format!("{op:?}");
    s.split(|c| c == '(' || c == ' ').next().unwrap_or(&s).to_string()
}

/// Stash a non-scalar StrykeValue, returning its registry id (for NativeFn).
fn reg_put(v: StrykeValue) -> u16 {
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.push(v);
        (r.len() - 1) as u16
    })
}

/// Retrieve a registry value by id.
fn reg_get(id: u16) -> StrykeValue {
    REGISTRY.with(|r| r.borrow().get(id as usize).cloned().unwrap_or(StrykeValue::UNDEF))
}

thread_local! {
    /// A runtime error raised inside [`native_ext_handler`] (which cannot return
    /// a `Result`). [`try_run_native`] drains it after the run and propagates it.
    static NATIVE_ERR: std::cell::RefCell<Option<StrykeError>> =
        const { std::cell::RefCell::new(None) };
}

fn set_native_err(e: StrykeError) {
    NATIVE_ERR.with(|c| {
        let mut slot = c.borrow_mut();
        if slot.is_none() {
            *slot = Some(e);
        }
    });
}

/// Convert a StrykeValue back to a native fusevm Value (scalar subset).
fn stryke_to_fusevm(v: &StrykeValue) -> fusevm::Value {
    if v.is_undef() {
        fusevm::Value::Undef
    } else if let Some(i) = v.as_integer() {
        fusevm::Value::Int(i)
    } else if let Some(f) = v.as_float() {
        fusevm::Value::Float(f)
    } else if let Some(s) = v.as_str() {
        fusevm::Value::Str(Arc::new(s))
    } else {
        // Non-scalar (closure / ref / regex / object): stash in the registry and
        // carry it as a NativeFn handle.
        fusevm::Value::NativeFn(reg_put(v.clone()))
    }
}

/// Numeric comparison matching strykelang's `int_cmp`: exact integer compare
/// when both operands are integers, else a float compare via `to_number`.
fn num_cmp(a: &StrykeValue, b: &StrykeValue, id: u16) -> bool {
    if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
        match id {
            nops::NUM_EQ => x == y,
            nops::NUM_NE => x != y,
            nops::NUM_LT => x < y,
            nops::NUM_GT => x > y,
            nops::NUM_LE => x <= y,
            _ => x >= y,
        }
    } else {
        let (x, y) = (a.to_number(), b.to_number());
        match id {
            nops::NUM_EQ => x == y,
            nops::NUM_NE => x != y,
            nops::NUM_LT => x < y,
            nops::NUM_GT => x > y,
            nops::NUM_LE => x <= y,
            _ => x >= y,
        }
    }
}

/// `<=>`: integer compare when both are integers, else float (matches vm.rs).
fn spaceship(a: &StrykeValue, b: &StrykeValue) -> i64 {
    if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
        (x > y) as i64 - (x < y) as i64
    } else {
        let (x, y) = (a.to_number(), b.to_number());
        if x < y {
            -1
        } else if x > y {
            1
        } else {
            0
        }
    }
}

/// Pop a fusevm Value and view it as a StrykeValue (scalars only on this path).
fn pop_stryke(vm: &mut fusevm::VM) -> StrykeValue {
    fusevm_to_stryke(&vm.pop()).unwrap_or(StrykeValue::UNDEF)
}

/// Stringify a fusevm Value the way strykelang stringifies a scalar, by routing
/// through `StrykeValue`'s `Display` (so float formatting etc. match vm.rs).
fn stryke_display(v: &fusevm::Value) -> String {
    match v {
        fusevm::Value::Int(n) => StrykeValue::integer(*n).to_string(),
        fusevm::Value::Float(f) => StrykeValue::float(*f).to_string(),
        fusevm::Value::Str(s) => (**s).clone(),
        other => fusevm_to_stryke(other)
            .map(|sv| sv.to_string())
            .unwrap_or_default(),
    }
}

/// Extension handler for strykelang's native-value Extended ops, installed on
/// the fusevm-only VM. Each op delegates to strykelang's own value semantics so
/// results match vm.rs.
fn native_ext_handler(vm: &mut fusevm::VM, id: u16, arg: u8) {
    match id {
        nops::CONCAT => {
            let b = vm.pop();
            let a = vm.pop();
            let mut s = stryke_display(&a);
            s.push_str(&stryke_display(&b));
            vm.push(fusevm::Value::Str(Arc::new(s)));
        }
        // String comparisons delegate to StrykeValue::str_eq / str_cmp (the same
        // methods vm.rs uses), so results match exactly. Push Perl bool (Int 1/0).
        nops::STR_EQ | nops::STR_NE | nops::STR_LT | nops::STR_GT | nops::STR_LE
        | nops::STR_GE => {
            use std::cmp::Ordering;
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            let truth = match id {
                nops::STR_EQ => a.str_eq(&b),
                nops::STR_NE => !a.str_eq(&b),
                nops::STR_LT => a.str_cmp(&b) == Ordering::Less,
                nops::STR_GT => a.str_cmp(&b) == Ordering::Greater,
                nops::STR_LE => matches!(a.str_cmp(&b), Ordering::Less | Ordering::Equal),
                _ => matches!(a.str_cmp(&b), Ordering::Greater | Ordering::Equal),
            };
            vm.push(fusevm::Value::Int(if truth { 1 } else { 0 }));
        }
        nops::NUM_EQ | nops::NUM_NE | nops::NUM_LT | nops::NUM_GT | nops::NUM_LE
        | nops::NUM_GE => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(if num_cmp(&a, &b, id) { 1 } else { 0 }));
        }
        nops::SPACESHIP => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(spaceship(&a, &b)));
        }
        nops::LOG_NOT => {
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(if a.is_true() { 0 } else { 1 }));
        }
        nops::TRUTHY => {
            let a = pop_stryke(vm);
            vm.push(fusevm::Value::Int(if a.is_true() { 1 } else { 0 }));
        }
        // `/` — integer quotient when both are integers and divisible, else
        // float; matches vm.rs's Div closure. Records div-by-zero in the error
        // slot and pushes Undef (the run is aborted after dispatch).
        nops::DIV => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            let result = if let (Some(x), Some(y)) = (a.as_integer(), b.as_integer()) {
                if y == 0 {
                    set_native_err(StrykeError::division_by_zero("Illegal division by zero", 0));
                    fusevm::Value::Undef
                } else if x % y == 0 {
                    fusevm::Value::Int(x / y)
                } else {
                    fusevm::Value::Float(x as f64 / y as f64)
                }
            } else {
                let d = b.to_number();
                if d == 0.0 {
                    set_native_err(StrykeError::division_by_zero("Illegal division by zero", 0));
                    fusevm::Value::Undef
                } else {
                    fusevm::Value::Float(a.to_number() / d)
                }
            };
            vm.push(result);
        }
        nops::MOD => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            let bi = b.to_int();
            if bi == 0 {
                set_native_err(StrykeError::division_by_zero("Illegal modulus zero", 0));
                vm.push(fusevm::Value::Undef);
            } else {
                vm.push(fusevm::Value::Int(crate::value::perl_mod_i64(a.to_int(), bi)));
            }
        }
        nops::POW => {
            let b = pop_stryke(vm);
            let a = pop_stryke(vm);
            vm.push(stryke_to_fusevm(&crate::value::compat_pow(&a, &b)));
        }
        // Name-scoped scalars — delegate to the interp (special-var resolution,
        // mutability checks, scope storage) via the host so semantics match
        // vm.rs exactly. `name_idx` was pushed by a preceding LoadInt.
        nops::GET_SCALAR => {
            let idx = vm.pop().to_int();
            let name = host_name(idx);
            let v = with_interp(|i| i.get_special_var(&name));
            vm.push(stryke_to_fusevm(&v));
        }
        nops::SET_SCALAR => {
            let idx = vm.pop().to_int();
            let val = pop_stryke(vm);
            let name = host_name(idx);
            let r = with_interp(|i| {
                i.maybe_invalidate_regex_capture_memo(&name);
                i.set_special_var(&name, &val)
            });
            if let Err(e) = r {
                set_native_err(e);
            }
        }
        nops::DECLARE_SCALAR => {
            let idx = vm.pop().to_int();
            let val = pop_stryke(vm);
            let name = host_name(idx);
            let r = with_interp(|i| i.scope.declare_scalar_frozen(&name, val, false, None));
            if let Err(e) = r {
                set_native_err(e);
            }
        }
        nops::GET_SCALAR_PLAIN => {
            let idx = vm.pop().to_int();
            let name = host_name(idx);
            let v = with_interp(|i| i.scope.get_scalar(&name));
            vm.push(stryke_to_fusevm(&v));
        }
        nops::SET_SCALAR_PLAIN => {
            let idx = vm.pop().to_int();
            let val = pop_stryke(vm);
            let name = host_name(idx);
            let r = with_interp(|i| {
                i.maybe_invalidate_regex_capture_memo(&name);
                i.scope.set_scalar(&name, val)
            });
            if let Err(e) = r {
                set_native_err(e);
            }
        }
        nops::SET_SCALAR_KEEP_PLAIN => {
            let idx = vm.pop().to_int();
            // KEEP: leave the assigned value on the stack (peek, don't pop).
            let val = fusevm_to_stryke(vm.peek()).unwrap_or(StrykeValue::UNDEF);
            let name = host_name(idx);
            let r = with_interp(|i| {
                i.maybe_invalidate_regex_capture_memo(&name);
                i.scope.set_scalar(&name, val)
            });
            if let Err(e) = r {
                set_native_err(e);
            }
        }
        nops::MAKE_ARRAY => {
            let n = vm.pop().to_int().max(0) as usize;
            let mut vals: Vec<fusevm::Value> = Vec::with_capacity(n);
            for _ in 0..n {
                vals.push(vm.pop());
            }
            vals.reverse(); // pops are last-to-first → restore source order
            // Perl list flatten: splice nested arrays in place.
            let mut flat: Vec<fusevm::Value> = Vec::with_capacity(n);
            for v in vals {
                match v {
                    fusevm::Value::Array(inner) => flat.extend(inner),
                    other => flat.push(other),
                }
            }
            vm.push(fusevm::Value::Array(flat));
        }
        nops::DECLARE_ARRAY => {
            let idx = vm.pop().to_int();
            let arr = vm.pop();
            let name = host_name(idx);
            let list: Vec<StrykeValue> = match arr {
                fusevm::Value::Array(items) => {
                    items.iter().map(|v| fusevm_to_stryke(v).unwrap_or(StrykeValue::UNDEF)).collect()
                }
                other => vec![fusevm_to_stryke(&other).unwrap_or(StrykeValue::UNDEF)],
            };
            with_interp(|i| i.scope.declare_array(&name, list));
        }
        nops::GET_ARRAY_ELEM => {
            let idx = vm.pop().to_int();
            let index = vm.pop().to_int();
            let name = host_name(idx);
            let v = with_interp(|i| array_elem_value(i, &name, index));
            vm.push(stryke_to_fusevm(&v));
        }
        nops::DECLARE_HASH => {
            let idx = vm.pop().to_int();
            let raw = vm.pop();
            let name = host_name(idx);
            let is_undef = matches!(raw, fusevm::Value::Undef);
            let items: Vec<StrykeValue> = match raw {
                fusevm::Value::Array(v) => {
                    v.iter().map(|x| fusevm_to_stryke(x).unwrap_or(StrykeValue::UNDEF)).collect()
                }
                fusevm::Value::Undef => Vec::new(),
                other => vec![fusevm_to_stryke(&other).unwrap_or(StrykeValue::UNDEF)],
            };
            with_interp(|i| {
                // `our %h;` (undef initializer, package-qualified) preserves
                // existing data; everything else folds the k/v pairs into a map.
                if is_undef && name.contains("::") {
                    let existing = i.scope.get_hash(&name);
                    i.scope.declare_hash(&name, existing);
                } else {
                    let mut map: IndexMap<String, StrykeValue> = IndexMap::new();
                    let mut k = 0;
                    while k + 1 < items.len() {
                        map.insert(items[k].to_string(), items[k + 1].clone());
                        k += 2;
                    }
                    i.scope.declare_hash(&name, map);
                }
            });
        }
        nops::GET_HASH_ELEM => {
            let idx = vm.pop().to_int();
            let key = pop_stryke(vm).to_string();
            let name = host_name(idx);
            let v = with_interp(|i| {
                i.touch_env_hash(&name);
                i.scope.get_hash_element(&name, &key)
            });
            vm.push(stryke_to_fusevm(&v));
        }
        nops::PRINT => do_print(vm, arg as usize, false),
        nops::SAY => do_print(vm, arg as usize, true),
        nops::CALL_SUB => {
            let _wa = vm.pop().to_int(); // wantarray byte (scalar context assumed)
            let argc = vm.pop().to_int().max(0) as usize;
            let name_idx = vm.pop().to_int();
            let mut args: Vec<StrykeValue> = Vec::with_capacity(argc);
            for _ in 0..argc {
                args.push(pop_stryke(vm));
            }
            args.reverse(); // pops are last-to-first → source order
            let name = host_name(name_idx);
            let r = with_interp(|i| {
                i.call_named_sub(&name, args, 0, crate::vm_helper::WantarrayCtx::Scalar)
            });
            match r {
                Ok(v) => vm.push(stryke_to_fusevm(&v)),
                Err(crate::vm_helper::FlowOrError::Error(e)) => {
                    set_native_err(e);
                    vm.push(fusevm::Value::Undef);
                }
                Err(_) => {
                    set_native_err(StrykeError::runtime("call: unexpected control flow", 0));
                    vm.push(fusevm::Value::Undef);
                }
            }
        }
        nops::REGEX_MATCH => {
            let pos_key_idx = vm.pop().to_int();
            let scalar_g = vm.pop().to_int() != 0;
            let flags_idx = vm.pop().to_int();
            let pat_idx = vm.pop().to_int();
            let val = pop_stryke(vm);
            let pattern =
                with_chunk(|c| c.constants.get(pat_idx as usize).map(|v| v.as_str_or_empty()).unwrap_or_default());
            let flags =
                with_chunk(|c| c.constants.get(flags_idx as usize).map(|v| v.as_str_or_empty()).unwrap_or_default());
            if val.is_iterator() {
                // Iterators aren't produced by any covered op yet, so this is
                // unreachable; error loudly rather than risk a silent divergence.
                set_native_err(StrykeError::runtime(
                    "regex match on an iterator is not yet supported on the native path",
                    0,
                ));
                vm.push(fusevm::Value::Undef);
            } else {
                let pos_key = if pos_key_idx == u16::MAX as i64 {
                    "_".to_string()
                } else {
                    with_chunk(|c| {
                        c.constants.get(pos_key_idx as usize).map(|v| v.as_str_or_empty()).unwrap_or_else(|| "_".into())
                    })
                };
                let s = val.into_string();
                let r =
                    with_interp(|i| i.regex_match_execute(s, &pattern, &flags, scalar_g, &pos_key, 0));
                match r {
                    Ok(v) => vm.push(stryke_to_fusevm(&v)),
                    Err(crate::vm_helper::FlowOrError::Error(e)) => {
                        set_native_err(e);
                        vm.push(fusevm::Value::Undef);
                    }
                    Err(_) => {
                        set_native_err(StrykeError::runtime("=~: unexpected control flow", 0));
                        vm.push(fusevm::Value::Undef);
                    }
                }
            }
        }
        nops::REGEX_SUBST => {
            let lvalue_idx = vm.pop().to_int();
            let flags_idx = vm.pop().to_int();
            let repl_idx = vm.pop().to_int();
            let pat_idx = vm.pop().to_int();
            let val = pop_stryke(vm);
            let (pattern, replacement, flags, target) = with_chunk(|c| {
                (
                    c.constants.get(pat_idx as usize).map(|v| v.as_str_or_empty()).unwrap_or_default(),
                    c.constants.get(repl_idx as usize).map(|v| v.as_str_or_empty()).unwrap_or_default(),
                    c.constants.get(flags_idx as usize).map(|v| v.as_str_or_empty()).unwrap_or_default(),
                    c.lvalues.get(lvalue_idx as usize).cloned(),
                )
            });
            if val.is_iterator() {
                set_native_err(StrykeError::runtime(
                    "s/// on an iterator is not yet supported on the native path",
                    0,
                ));
                vm.push(fusevm::Value::Undef);
            } else if let Some(target) = target {
                let s = val.into_string();
                let r = with_interp(|i| {
                    i.regex_subst_execute(s, &pattern, &replacement, &flags, &target, 0)
                });
                match r {
                    Ok(v) => vm.push(stryke_to_fusevm(&v)),
                    Err(crate::vm_helper::FlowOrError::Error(e)) => {
                        set_native_err(e);
                        vm.push(fusevm::Value::Undef);
                    }
                    Err(_) => {
                        set_native_err(StrykeError::runtime("s///: unexpected control flow", 0));
                        vm.push(fusevm::Value::Undef);
                    }
                }
            } else {
                set_native_err(StrykeError::runtime("s///: bad lvalue index", 0));
                vm.push(fusevm::Value::Undef);
            }
        }
        // Perl `++` via perl_inc (magic increment: numbers and "az"->"ba").
        nops::INC_SLOT_VOID => {
            let slot = vm.pop().to_int() as u8;
            with_interp(|i| {
                let cur = i.scope.get_scalar_slot(slot);
                let next = crate::vm_helper::perl_inc(&cur);
                i.scope.set_scalar_slot(slot, next);
            });
        }
        nops::POST_INC_SLOT => {
            let slot = vm.pop().to_int() as u8;
            let old = with_interp(|i| {
                let old = i.scope.get_scalar_slot(slot);
                let next = crate::vm_helper::perl_inc(&old);
                i.scope.set_scalar_slot(slot, next);
                old
            });
            vm.push(stryke_to_fusevm(&old));
        }
        // Scalar slots in interp.scope (so closures can capture `my` locals).
        nops::SLOT_GET => {
            let slot = vm.pop().to_int() as u8;
            let v = with_interp(|i| i.scope.get_scalar_slot(slot));
            vm.push(stryke_to_fusevm(&v));
        }
        nops::SLOT_SET => {
            let slot = vm.pop().to_int() as u8;
            let val = pop_stryke(vm);
            with_interp(|i| i.scope.set_scalar_slot(slot, val));
        }
        nops::SLOT_SET_KEEP => {
            let slot = vm.pop().to_int() as u8;
            // KEEP: leave the value on the stack (peek), store a copy.
            let val = fusevm_to_stryke(vm.peek()).unwrap_or(StrykeValue::UNDEF);
            with_interp(|i| i.scope.set_scalar_slot(slot, val));
        }
        nops::SLOT_DECLARE => {
            let name_idx = vm.pop().to_int();
            let slot = vm.pop().to_int() as u8;
            let val = pop_stryke(vm);
            let name = if name_idx == u16::MAX as i64 {
                None
            } else {
                Some(host_name(name_idx))
            };
            with_interp(|i| i.scope.declare_scalar_slot(slot, val, name.as_deref()));
        }
        nops::MAKE_CODEREF => {
            let sig_idx = vm.pop().to_int() as usize;
            let block_idx = vm.pop().to_int() as usize;
            let parts = with_chunk(|c| {
                (c.blocks.get(block_idx).cloned(), c.code_ref_sigs.get(sig_idx).cloned())
            });
            match parts {
                (Some(block), Some(params)) => {
                    let captured = with_interp(|i| i.scope.capture());
                    let coderef = StrykeValue::code_ref(Arc::new(crate::value::StrykeSub {
                        name: "__ANON__".to_string(),
                        params,
                        body: block,
                        closure_env: Some(captured),
                        prototype: None,
                        fib_like: None,
                        return_type: None,
                    }));
                    vm.push(stryke_to_fusevm(&coderef));
                }
                _ => {
                    set_native_err(StrykeError::runtime("MakeCodeRef: bad block/sig index", 0));
                    vm.push(fusevm::Value::Undef);
                }
            }
        }
        nops::ARROW_CALL => {
            let want = crate::vm_helper::WantarrayCtx::from_byte(arg);
            // Multiple args ride as a Value::Array (built by MakeArray); a single
            // arg rides as a scalar. Flatten either to the arg list.
            let args_raw = vm.pop();
            let args: Vec<StrykeValue> = match args_raw {
                fusevm::Value::Array(items) => {
                    items.iter().map(|v| fusevm_to_stryke(v).unwrap_or(StrykeValue::UNDEF)).collect()
                }
                other => vec![fusevm_to_stryke(&other).unwrap_or(StrykeValue::UNDEF)],
            };
            let mut callee = pop_stryke(vm);
            // Auto-deref a scalar ref so `$f->()` works when $f holds a ref.
            if let Some(inner) = callee.as_scalar_ref() {
                callee = inner.read().clone();
            }
            if let Some(sub) = callee.as_code_ref() {
                let r = with_interp(|i| i.call_sub(&sub, args, want, 0));
                match r {
                    Ok(v) => vm.push(stryke_to_fusevm(&v)),
                    Err(crate::vm_helper::FlowOrError::Error(e)) => {
                        set_native_err(e);
                        vm.push(fusevm::Value::Undef);
                    }
                    Err(_) => {
                        set_native_err(StrykeError::runtime("->: unexpected control flow", 0));
                        vm.push(fusevm::Value::Undef);
                    }
                }
            } else {
                set_native_err(StrykeError::runtime(
                    "Not a CODE reference in arrow call",
                    0,
                ));
                vm.push(fusevm::Value::Undef);
            }
        }
        _ => {}
    }
}

/// Convert a stryke constant to a native fusevm Value, if it is in the Phase-1
/// subset (integer/float). Heap values (strings/arrays/hashes/closures/…) return
/// `None` and are handled in later phases.
fn const_to_fusevm(v: &StrykeValue) -> Option<fusevm::Value> {
    if let Some(i) = v.as_integer() {
        Some(fusevm::Value::Int(i))
    } else if let Some(f) = v.as_float() {
        Some(fusevm::Value::Float(f))
    } else {
        v.as_str().map(|s| fusevm::Value::Str(Arc::new(s)))
    }
}

/// Convert a fusevm result Value back to a StrykeValue (Phase-1 subset).
fn fusevm_to_stryke(v: &fusevm::Value) -> Option<StrykeValue> {
    match v {
        fusevm::Value::Int(n) => Some(StrykeValue::integer(*n)),
        fusevm::Value::Float(f) => Some(StrykeValue::float(*f)),
        fusevm::Value::Str(s) => Some(StrykeValue::string((**s).clone())),
        fusevm::Value::Undef => Some(StrykeValue::UNDEF),
        fusevm::Value::NativeFn(id) => Some(reg_get(*id)),
        _ => None,
    }
}

/// Attempt to run `chunk` entirely on `fusevm::VM` with native Values. Returns
/// `Some(result)` when the whole program is in the covered subset, else `None`
/// (the caller then runs it on `crate::vm`). `interp` is unused in Phase 1 (no
/// vars/closures/host yet) but threaded for later phases.
pub fn try_run_native(chunk: &Chunk, interp: &mut VMHelper) -> Option<StrykeResult<StrykeValue>> {
    let mut b = fusevm::ChunkBuilder::new();
    // Map each source op index to the fusevm op index its lowering starts at, so
    // jump targets (absolute source indices) can be remapped after lowering.
    let mut src_to_dst: Vec<usize> = Vec::with_capacity(chunk.ops.len() + 1);
    // (fusevm jump op index, source target index) pairs, patched in pass 2.
    let mut jump_fixups: Vec<(usize, usize)> = Vec::new();
    // Halt jump indices, patched to end-of-chunk (so a top-level Halt skips any
    // sub bodies appended after it rather than falling through into them).
    let mut halt_fixups: Vec<usize> = Vec::new();
    for op in &chunk.ops {
        src_to_dst.push(b.current_pos());
        match op {
            Op::Nop => {
                b.emit(fusevm::Op::Nop, 0);
            }
            // Phase marker for BEGIN/END/AOP ordering; irrelevant to a pure
            // arithmetic result. Phase-dependent programs use AOP ops outside
            // this subset, which abort to `None` below.
            Op::SetGlobalPhase(_) => {}
            Op::LoadInt(n) => {
                b.emit(fusevm::Op::LoadInt(*n), 0);
            }
            Op::LoadFloat(f) => {
                b.emit(fusevm::Op::LoadFloat(*f), 0);
            }
            Op::LoadUndef => {
                b.emit(fusevm::Op::LoadUndef, 0);
            }
            Op::LoadConst(idx) => {
                let sv = chunk.constants.get(*idx as usize)?;
                let fv = const_to_fusevm(sv)?;
                let fi = b.add_constant(fv);
                b.emit(fusevm::Op::LoadConst(fi), 0);
            }
            Op::Pop => {
                b.emit(fusevm::Op::Pop, 0);
            }
            Op::Add => {
                b.emit(fusevm::Op::Add, 0);
            }
            Op::Sub => {
                b.emit(fusevm::Op::Sub, 0);
            }
            Op::Mul => {
                b.emit(fusevm::Op::Mul, 0);
            }
            Op::Div => {
                b.emit(fusevm::Op::Extended(nops::DIV, 0), 0);
            }
            Op::Mod => {
                b.emit(fusevm::Op::Extended(nops::MOD, 0), 0);
            }
            Op::Pow => {
                b.emit(fusevm::Op::Extended(nops::POW, 0), 0);
            }
            // Name-scoped scalars: push the name index, then the Extended op
            // resolves + delegates to the interp via the host.
            Op::GetScalar(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::GET_SCALAR, 0), 0);
            }
            Op::SetScalar(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SET_SCALAR, 0), 0);
            }
            Op::DeclareScalar(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::DECLARE_SCALAR, 0), 0);
            }
            Op::GetScalarPlain(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::GET_SCALAR_PLAIN, 0), 0);
            }
            Op::SetScalarPlain(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SET_SCALAR_PLAIN, 0), 0);
            }
            Op::SetScalarKeepPlain(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SET_SCALAR_KEEP_PLAIN, 0), 0);
            }
            // Arrays: push the count / name index, then the Extended op.
            Op::MakeArray(n) => {
                b.emit(fusevm::Op::LoadInt(*n as i64), 0);
                b.emit(fusevm::Op::Extended(nops::MAKE_ARRAY, 0), 0);
            }
            Op::DeclareArray(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::DECLARE_ARRAY, 0), 0);
            }
            Op::GetArrayElem(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::GET_ARRAY_ELEM, 0), 0);
            }
            Op::DeclareHash(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::DECLARE_HASH, 0), 0);
            }
            Op::GetHashElem(idx) => {
                b.emit(fusevm::Op::LoadInt(*idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::GET_HASH_ELEM, 0), 0);
            }
            // print/say to the DEFAULT handle only (the common case); a named
            // handle (`print $fh ...`) falls back to vm.rs for now. The arg
            // count rides in the Extended op's `arg` byte.
            Op::Print(None, argc) => {
                b.emit(fusevm::Op::Extended(nops::PRINT, *argc), 0);
            }
            Op::Say(None, argc) => {
                b.emit(fusevm::Op::Extended(nops::SAY, *argc), 0);
            }
            Op::Negate => {
                b.emit(fusevm::Op::Negate, 0);
            }
            // Perl `.` concatenation → native-value Extended op (handler
            // stringifies scalars via strykelang's Display).
            Op::Concat => {
                b.emit(fusevm::Op::Extended(nops::CONCAT, 0), 0);
            }
            Op::StrEq => {
                b.emit(fusevm::Op::Extended(nops::STR_EQ, 0), 0);
            }
            Op::StrNe => {
                b.emit(fusevm::Op::Extended(nops::STR_NE, 0), 0);
            }
            Op::StrLt => {
                b.emit(fusevm::Op::Extended(nops::STR_LT, 0), 0);
            }
            Op::StrGt => {
                b.emit(fusevm::Op::Extended(nops::STR_GT, 0), 0);
            }
            Op::StrLe => {
                b.emit(fusevm::Op::Extended(nops::STR_LE, 0), 0);
            }
            Op::StrGe => {
                b.emit(fusevm::Op::Extended(nops::STR_GE, 0), 0);
            }
            Op::NumEq => {
                b.emit(fusevm::Op::Extended(nops::NUM_EQ, 0), 0);
            }
            Op::NumNe => {
                b.emit(fusevm::Op::Extended(nops::NUM_NE, 0), 0);
            }
            Op::NumLt => {
                b.emit(fusevm::Op::Extended(nops::NUM_LT, 0), 0);
            }
            Op::NumGt => {
                b.emit(fusevm::Op::Extended(nops::NUM_GT, 0), 0);
            }
            Op::NumLe => {
                b.emit(fusevm::Op::Extended(nops::NUM_LE, 0), 0);
            }
            Op::NumGe => {
                b.emit(fusevm::Op::Extended(nops::NUM_GE, 0), 0);
            }
            Op::Spaceship => {
                b.emit(fusevm::Op::Extended(nops::SPACESHIP, 0), 0);
            }
            Op::LogNot => {
                b.emit(fusevm::Op::Extended(nops::LOG_NOT, 0), 0);
            }
            // Scalar locals: strykelang stores them in `interp.scope` slots; on
            // the native path they live in the fusevm frame's slots instead
            // (self-consistent within the run — Declare/Set/Get use the same
            // storage). `set_slot` auto-grows, so no pre-sizing is needed. The
            // declared name is only symbolic and is dropped.
            Op::DeclareScalarSlot(slot, name_idx) => {
                b.emit(fusevm::Op::LoadInt(*slot as i64), 0);
                b.emit(fusevm::Op::LoadInt(*name_idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_DECLARE, 0), 0);
            }
            Op::GetScalarSlot(slot) => {
                b.emit(fusevm::Op::LoadInt(*slot as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_GET, 0), 0);
            }
            Op::SetScalarSlot(slot) => {
                b.emit(fusevm::Op::LoadInt(*slot as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_SET, 0), 0);
            }
            Op::SetScalarSlotKeep(slot) => {
                b.emit(fusevm::Op::LoadInt(*slot as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_SET_KEEP, 0), 0);
            }
            // Fused superinstructions (loops/compound-assign). Lowered to their
            // unfused universal-op equivalents — correct, and fusevm's block JIT
            // re-fuses these very patterns to machine code.
            Op::AddAssignSlotSlotVoid(d, s) => {
                b.emit(fusevm::Op::LoadInt(*d as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_GET, 0), 0);
                b.emit(fusevm::Op::LoadInt(*s as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_GET, 0), 0);
                b.emit(fusevm::Op::Add, 0);
                b.emit(fusevm::Op::LoadInt(*d as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_SET, 0), 0);
            }
            // `++slot` uses Perl magic increment (perl_inc), not numeric +1, so
            // string-increment (`$s++` on "az") matches vm.rs.
            Op::PreIncSlotVoid(s) => {
                b.emit(fusevm::Op::LoadInt(*s as i64), 0);
                b.emit(fusevm::Op::Extended(nops::INC_SLOT_VOID, 0), 0);
            }
            Op::PostIncSlot(s) => {
                b.emit(fusevm::Op::LoadInt(*s as i64), 0);
                b.emit(fusevm::Op::Extended(nops::POST_INC_SLOT, 0), 0);
            }
            // `slot < int` then conditional jump. The NUM_LT Extended op yields
            // Int 0/1, so JumpIfFalse needs no TRUTHY normalization here.
            Op::SlotLtIntJumpIfFalse(slot, int, target) => {
                b.emit(fusevm::Op::LoadInt(*slot as i64), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_GET, 0), 0);
                b.emit(fusevm::Op::LoadInt(*int as i64), 0);
                b.emit(fusevm::Op::Extended(nops::NUM_LT, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfFalse(0), 0);
                jump_fixups.push((pos, *target));
            }
            // `++slot; if slot < limit goto target` — the for-loop trailing
            // increment+test+backjump, unfused.
            Op::SlotIncLtIntJumpBack(slot, limit, target) => {
                let s = *slot as i64;
                b.emit(fusevm::Op::LoadInt(s), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_GET, 0), 0);
                b.emit(fusevm::Op::LoadInt(1), 0);
                b.emit(fusevm::Op::Add, 0);
                b.emit(fusevm::Op::LoadInt(s), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_SET, 0), 0);
                b.emit(fusevm::Op::LoadInt(s), 0);
                b.emit(fusevm::Op::Extended(nops::SLOT_GET, 0), 0);
                b.emit(fusevm::Op::LoadInt(*limit as i64), 0);
                b.emit(fusevm::Op::Extended(nops::NUM_LT, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfTrue(0), 0);
                jump_fixups.push((pos, *target));
            }
            // Top-level terminator. Jump to end-of-chunk so any sub bodies
            // appended after Halt are skipped (they run via call delegation, not
            // by falling through). A plain fusevm `Return` is wrong here: at the
            // root frame it pops the frame and resets ip, re-running everything.
            Op::Halt => {
                let pos = b.emit(fusevm::Op::Jump(0), 0);
                halt_fixups.push(pos);
            }
            // Sub/closure-body terminators: those bodies run via call delegation
            // (the interp), so in the fusevm chunk they're unreachable after the
            // Halt jump — emit nothing.
            Op::Return | Op::ReturnValue | Op::BlockReturnValue => {}
            // Static user-sub call: push name index, argc, wantarray, then the
            // Extended op delegates the whole call to the interp.
            Op::CallStaticSubId(_sid, name_idx, argc, wa) => {
                b.emit(fusevm::Op::LoadInt(*name_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*argc as i64), 0);
                b.emit(fusevm::Op::LoadInt(*wa as i64), 0);
                b.emit(fusevm::Op::Extended(nops::CALL_SUB, 0), 0);
            }
            // Closures: build the coderef (block + sig indices), call via arrow.
            Op::MakeCodeRef(block_idx, sig_idx) => {
                b.emit(fusevm::Op::LoadInt(*block_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*sig_idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::MAKE_CODEREF, 0), 0);
            }
            Op::ArrowCall(wa) => {
                b.emit(fusevm::Op::Extended(nops::ARROW_CALL, *wa), 0);
            }
            Op::RegexMatch(pat_idx, flags_idx, scalar_g, pos_key_idx) => {
                b.emit(fusevm::Op::LoadInt(*pat_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*flags_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*scalar_g as i64), 0);
                b.emit(fusevm::Op::LoadInt(*pos_key_idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::REGEX_MATCH, 0), 0);
            }
            Op::RegexSubst(pat_idx, repl_idx, flags_idx, lvalue_idx) => {
                b.emit(fusevm::Op::LoadInt(*pat_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*repl_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*flags_idx as i64), 0);
                b.emit(fusevm::Op::LoadInt(*lvalue_idx as i64), 0);
                b.emit(fusevm::Op::Extended(nops::REGEX_SUBST, 0), 0);
            }
            // Control flow (pop variants). Emit the fusevm jump with a
            // placeholder target recorded for fixup. Conditional jumps first
            // normalize the condition to Perl truthiness (Int 0/1) via TRUTHY,
            // since fusevm's native truthiness differs from strykelang's.
            // The Keep variants (`&&`/`||`/ternary short-circuit) peek and are
            // not yet covered → they hit `_ => return None` and fall back.
            Op::Jump(t) => {
                let pos = b.emit(fusevm::Op::Jump(0), 0);
                jump_fixups.push((pos, *t));
            }
            Op::JumpIfFalse(t) => {
                b.emit(fusevm::Op::Extended(nops::TRUTHY, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfFalse(0), 0);
                jump_fixups.push((pos, *t));
            }
            Op::JumpIfTrue(t) => {
                b.emit(fusevm::Op::Extended(nops::TRUTHY, 0), 0);
                let pos = b.emit(fusevm::Op::JumpIfTrue(0), 0);
                jump_fixups.push((pos, *t));
            }
            // Anything else is outside the covered subset → fall back to vm.rs.
            other => {
                // Coverage measurement: STRYKE_FUSEVM_TRACE logs the first
                // uncovered op so gaps can be ranked by frequency.
                if std::env::var_os("STRYKE_FUSEVM_TRACE").is_some() {
                    eprintln!("FUSEVM_UNCOVERED {}", op_name(other));
                }
                return None;
            }
        }
    }
    // End sentinel so a jump to "one past the last op" resolves to program end.
    let end = b.current_pos();
    src_to_dst.push(end);
    for (pos, target) in jump_fixups {
        let dst = *src_to_dst.get(target)?;
        b.patch_jump(pos, dst);
    }
    // Halt jumps go to end-of-chunk (skipping any appended sub bodies).
    for pos in halt_fixups {
        b.patch_jump(pos, end);
    }

    let fchunk = b.build();
    let mut vm = fusevm::VM::new(fchunk);
    vm.set_extension_handler(Box::new(native_ext_handler));
    NATIVE_ERR.with(|c| *c.borrow_mut() = None);
    REGISTRY.with(|r| r.borrow_mut().clear());
    // Make the live interp + chunk (names / blocks / code_ref_sigs) reachable
    // from handlers for the run's duration. The guard brackets `vm.run()`
    // exactly (synchronous), so the raw pointers are valid throughout.
    let outcome = {
        let _host = HostGuard::enter(interp, chunk);
        vm.run()
    };
    // A runtime error raised inside the handler (e.g. division by zero) takes
    // precedence over the VM's own result.
    if let Some(e) = NATIVE_ERR.with(|c| c.borrow_mut().take()) {
        return Some(Err(e));
    }
    match outcome {
        fusevm::VMResult::Ok(v) => match fusevm_to_stryke(&v) {
            Some(sv) => Some(Ok(sv)),
            // Result type outside the subset; let vm.rs handle it (the covered
            // ops have no side effects, so re-running is safe).
            None => None,
        },
        fusevm::VMResult::Halted => Some(Ok(StrykeValue::UNDEF)),
        fusevm::VMResult::Error(e) => Some(Err(StrykeError::runtime(e, 0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile `code` and run it on the fusevm-only path; `None` means the
    /// program is outside the covered subset.
    fn native(code: &str) -> Option<StrykeResult<StrykeValue>> {
        let program = crate::parse(code).expect("parse");
        let mut interp = VMHelper::new();
        // Mirror the real path (try_vm_execute): register top-level declarations
        // (subs, etc.) before running, so call delegation can resolve them.
        interp.prepare_program_top_level(&program).expect("prepare");
        let chunk = crate::compiler::Compiler::new()
            .compile_program(&program)
            .expect("compile");
        try_run_native(&chunk, &mut interp)
    }

    /// Native path must agree with the normal vm.rs path on the returned value
    /// for arithmetic programs.
    fn assert_parity_int(code: &str, expect: i64) {
        let vm = crate::run(code).expect("vm run");
        assert_eq!(vm.as_integer(), Some(expect), "vm.rs value for `{code}`");
        let nat = native(code)
            .unwrap_or_else(|| panic!("`{code}` not covered by native path"))
            .expect("native run");
        assert_eq!(nat.as_integer(), Some(expect), "native value for `{code}`");
    }

    /// Native path must agree with vm.rs on string-valued results.
    fn assert_parity_str(code: &str, expect: &str) {
        let vm = crate::run(code).expect("vm run");
        assert_eq!(vm.as_str().as_deref(), Some(expect), "vm.rs value for `{code}`");
        let nat = native(code)
            .unwrap_or_else(|| panic!("`{code}` not covered by native path"))
            .expect("native run");
        assert_eq!(nat.as_str().as_deref(), Some(expect), "native value for `{code}`");
    }

    #[test]
    fn native_integer_arithmetic_matches_vm() {
        assert_parity_int("1 + 2 * 3", 7);
        assert_parity_int("10 - 4 - 3", 3);
        assert_parity_int("2 * 3 * 4", 24);
        assert_parity_int("-(2 + 3)", -5);
    }

    #[test]
    fn native_scalar_concat_matches_vm() {
        assert_parity_str("\"a\" . \"b\"", "ab");
        assert_parity_str("\"x\" . 3", "x3");
        assert_parity_str("1 . 2", "12");
        assert_parity_str("\"sum=\" . (2 + 3)", "sum=5");
    }

    #[test]
    fn native_string_compares_match_vm() {
        assert_parity_int("\"a\" eq \"a\"", 1);
        assert_parity_int("\"a\" eq \"b\"", 0);
        assert_parity_int("\"a\" ne \"b\"", 1);
        assert_parity_int("\"a\" lt \"b\"", 1);
        assert_parity_int("\"b\" gt \"a\"", 1);
        assert_parity_int("\"a\" le \"a\"", 1);
        assert_parity_int("\"b\" ge \"a\"", 1);
    }

    #[test]
    fn native_numeric_compares_match_vm() {
        assert_parity_int("1 < 2", 1);
        assert_parity_int("2 < 1", 0);
        assert_parity_int("3 == 3", 1);
        assert_parity_int("3 != 4", 1);
        assert_parity_int("5 >= 5", 1);
        assert_parity_int("2 <= 1", 0);
        assert_parity_int("1.5 < 2.5", 1);
    }

    #[test]
    fn native_spaceship_and_lognot_match_vm() {
        assert_parity_int("1 <=> 2", -1);
        assert_parity_int("2 <=> 2", 0);
        assert_parity_int("3 <=> 1", 1);
        assert_parity_int("!0", 1);
        assert_parity_int("!5", 0);
    }

    #[test]
    fn native_if_else_control_flow_matches_vm() {
        assert_parity_int("if (1 < 2) { 10 } else { 20 }", 10);
        assert_parity_int("if (2 < 1) { 10 } else { 20 }", 20);
        // Perl truthiness: the string "0" is false (fusevm's native truthiness
        // would treat it true) — exercises the TRUTHY normalization.
        assert_parity_int("if (\"0\") { 1 } else { 2 }", 2);
        assert_parity_int("if (\"x\") { 1 } else { 2 }", 1);
    }

    #[test]
    fn native_scalar_variables_match_vm() {
        assert_parity_int("my $x = 5; $x + 1", 6);
        assert_parity_int("my $x = 5; my $y = 10; $x + $y", 15);
        assert_parity_int("my $x = 5; $x = $x * 2; $x", 10);
        assert_parity_str("my $s = \"hi\"; $s . \"!\"", "hi!");
    }

    /// Native path must agree with vm.rs on the value's display form (covers
    /// any scalar type — int, float, string).
    fn assert_parity_display(code: &str, expect: &str) {
        let vm = crate::run(code).expect("vm run");
        assert_eq!(vm.to_string(), expect, "vm.rs value for `{code}`");
        let nat = native(code)
            .unwrap_or_else(|| panic!("`{code}` not covered by native path"))
            .expect("native run");
        assert_eq!(nat.to_string(), expect, "native value for `{code}`");
    }

    #[test]
    fn native_div_mod_pow_match_vm() {
        // Display-based (native must equal vm.rs) — avoids int-vs-float
        // assumptions (Perl `**` yields a float; `/` is int only when divisible).
        for code in ["10 / 2", "7 / 2", "17 % 5", "(-7) % 3", "2 ** 10", "9 ** 0.5"] {
            let expect = crate::run(code).expect("vm run").to_string();
            assert_parity_display(code, &expect);
        }
    }

    #[test]
    fn native_undef_and_for_loops_match_vm() {
        assert_parity_display("my $x; $x", &crate::run("my $x; $x").unwrap().to_string());
        // C-style for loop (trailing SlotIncLtIntJumpBack)
        assert_parity_int(
            "my $s = 0; for (my $i = 0; $i < 5; $i++) { $s = $s + $i } $s",
            10,
        );
    }

    #[test]
    fn native_regex_subst_matches_vm() {
        assert_parity_str("my $s = \"foo\"; $s =~ s/o/0/g; $s", "f00");
        assert_parity_str("my $s = \"hello\"; $s =~ s/l/L/; $s", "heLlo");
        assert_parity_int("my $s = \"aaa\"; my $n = ($s =~ s/a/b/g); $n", 3);
    }

    #[test]
    fn native_regex_match_matches_vm() {
        assert_parity_int("my $x = \"hello\"; $x =~ /ell/ ? 1 : 0", 1);
        assert_parity_int("\"hello\" =~ /xyz/ ? 1 : 0", 0);
        assert_parity_int("\"abc123\" =~ /\\d+/ ? 1 : 0", 1);
        assert_parity_int("\"Hello\" =~ /hello/i ? 1 : 0", 1); // case-insensitive flag
    }

    #[test]
    fn native_closures_match_vm() {
        assert_parity_int("my $f = fn($x) { $x * 2 }; $f->(21)", 42);
        assert_parity_int("my $add = fn($a, $b) { $a + $b }; $add->(2, 3)", 5);
        // closure capturing a lexical
        assert_parity_int("my $n = 10; my $g = fn($x) { $x + $n }; $g->(5)", 15);
    }

    #[test]
    fn native_function_calls_match_vm() {
        assert_parity_int("fn add($a, $b) { $a + $b } add(2, 3)", 5);
        assert_parity_int("fn dbl($n) { $n * 2 } dbl(21)", 42);
        assert_parity_int("fn add($a, $b) { $a + $b } add(2, 3) + add(10, 20)", 35);
        // recursion (non-builtin name)
        assert_parity_int(
            "fn myfact($n) { if ($n <= 1) { 1 } else { $n * myfact($n - 1) } } myfact(5)",
            120,
        );
    }

    #[test]
    fn native_string_interpolation_matches_vm() {
        // Interpolation compiles to LoadConst + GetScalar + Concat — already
        // covered by existing ops; this pins that they compose correctly.
        assert_parity_str("my $x = 5; \"sum: $x\"", "sum: 5");
        assert_parity_str("my $n = 3; \"n=${n}!\"", "n=3!");
        assert_parity_str("my $a = 2; my $b = 3; \"$a+$b\"", "2+3");
    }

    #[test]
    fn native_hashes_match_vm() {
        assert_parity_int("my %h = (\"a\", 1, \"b\", 2); $h{\"a\"}", 1);
        assert_parity_int("my %h = (\"a\", 1, \"b\", 2); $h{\"b\"}", 2);
        assert_parity_int("my %h = (\"x\", 10); $h{\"x\"} + 5", 15);
        // missing key → undef (display ""), checked against vm.rs
        assert_parity_display("my %h = (\"a\", 1); $h{\"z\"}", &crate::run("my %h = (\"a\", 1); $h{\"z\"}").unwrap().to_string());
    }

    #[test]
    fn native_arrays_match_vm() {
        assert_parity_int("my @a = (1, 2, 3); $a[1]", 2);
        assert_parity_int("my @a = (10, 20, 30); $a[0] + $a[2]", 40);
        assert_parity_int("my @a = (1, 2, 3); $a[-1]", 3); // negative index
        assert_parity_int("my @a = (5, 6, 7, 8); $a[3] - $a[0]", 3);
    }

    #[test]
    fn native_name_scoped_scalars_match_vm() {
        // `our`/package/global scalars go through the interp host (scope), not
        // fusevm frame slots like `my` locals do.
        assert_parity_int("our $x = 5; $x + 1", 6);
        assert_parity_int("$x = 5; $x + 1", 6);
        assert_parity_int("$g = 3; $g * 2", 6);
        assert_parity_int("$a = 4; $b = 10; $a + $b", 14);
    }

    #[test]
    fn native_while_loops_match_vm() {
        assert_parity_int(
            "my $s = 0; my $i = 1; while ($i <= 10) { $s = $s + $i; $i = $i + 1 } $s",
            55,
        );
        // if-with-comparison compiles to the fused SlotLtIntJumpIfFalse.
        assert_parity_int("my $x = 1; if ($x < 5) { $x = 9 } $x", 9);
        assert_parity_int("my $x = 7; if ($x < 5) { $x = 9 } $x", 7);
    }
}
