//! Hand-crafted edge-case pins for `stryke::getopts::builtin_getopts`.
//!
//! Each test below targets a specific bug class in the argv parser that the
//! existing in-module tests (37 of them) do NOT cover. The goal is to pin
//! current observable behavior so that a future refactor that "fixes" or
//! changes any of these surface shapes must consciously update the test —
//! catching unintended drift.
//!
//! Bug classes covered:
//!
//! 1. `Required` consumes the next argv token *unconditionally*, including
//!    the `--` option-terminator. This silently violates the universal
//!    "`--` ends option parsing" contract — the `--` ends up stored as the
//!    flag's string value instead of marking the boundary.
//!
//! 2. Duplicate aliases across two distinct specs (same short letter `x`
//!    listed under both `verbose|x` and `debug|x`) are NOT rejected by the
//!    duplicate-canonical-name check. The first matching spec wins
//!    silently, so `-x` binds to whichever spec was registered first.
//!
//! 3. `=s%` hash kv accepts an empty key (`-D =val`) and stores it under
//!    the empty-string key — the `split_hash_kv` helper finds `=` at byte
//!    index 0 and returns `("", "val")` with no validation.

use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use stryke::getopts::builtin_getopts;
use stryke::value::StrykeValue;
use stryke::vm_helper::VMHelper;

fn argv_ref(items: &[&str]) -> StrykeValue {
    let v: Vec<StrykeValue> = items
        .iter()
        .map(|s| StrykeValue::string((*s).to_string()))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn specs_ref(items: &[&str]) -> StrykeValue {
    let v: Vec<StrykeValue> = items
        .iter()
        .map(|s| StrykeValue::string((*s).to_string()))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn hget_string(result: &StrykeValue, key: &str) -> Option<String> {
    result
        .as_hash_ref()
        .expect("getopts result is a hash ref")
        .read()
        .get(key)
        .map(|v| v.to_string())
}

fn hget_int(result: &StrykeValue, key: &str) -> Option<i64> {
    result
        .as_hash_ref()
        .expect("getopts result is a hash ref")
        .read()
        .get(key)
        .map(|v| v.to_int())
}

fn leftover_strings(argv: &StrykeValue) -> Vec<String> {
    argv.as_array_ref()
        .expect("argv is an array ref")
        .read()
        .iter()
        .map(|v| v.to_string())
        .collect()
}

/// `Required(Str)` greedily consumes whatever sits in argv at the next slot,
/// including `--`. After parsing `["--file", "--", "rest"]` with spec
/// `file=s`, the parser stores `file => "--"` and treats `rest` as the only
/// positional. The terminator semantic — "no token after `--` is ever an
/// option value" — is silently broken.
///
/// This is NOT a mirror test: the existing `missing_required_value_errors`
/// only checks the empty-argv-tail case, and `double_dash_terminator` only
/// tests the bool-flag case (`--verbose -- ...`) where consume_option never
/// touches the next token. No existing test pairs `Required(...)` with a
/// trailing `--`. A single-line fix in `consume_option` (checking
/// `input[i] == "--"` before the unconditional `input[i].clone()` at
/// strykelang/getopts.rs:583) would flip this test, so it pins the
/// observable surface either way: today it asserts the buggy "--" capture;
/// a future fix would have to update the assertion intentionally.
#[test]
fn required_string_greedily_consumes_double_dash_terminator() {
    let mut interp = VMHelper::new();
    let argv = argv_ref(&["--file", "--", "rest"]);
    let specs = specs_ref(&["file=s"]);

    let out = builtin_getopts(&mut interp, &[argv.clone(), specs], 1)
        .expect("getopts should not error here");

    // Current behavior: `--` is captured as the value of --file. This is
    // a bug-class pin — the standard contract is that `--` ends option
    // parsing, but `Required` doesn't check for it.
    assert_eq!(
        hget_string(&out, "file").as_deref(),
        Some("--"),
        "Required(Str) currently swallows the `--` terminator as a value; \
         if this changes (either to error 'requires a value' or to treat \
         `rest` as the value), the parser semantics around `--` have shifted"
    );

    // And the rest of argv after `--` becomes leftover positional —
    // which is consistent with `--` being eaten, not honored.
    let left = leftover_strings(&argv);
    assert_eq!(
        left,
        vec!["rest".to_string()],
        "with `--` consumed as a value, only `rest` survives as positional"
    );
}

/// Two distinct specs sharing the same short alias (`x`) pass the
/// duplicate-canonical-name check at strykelang/getopts.rs:970-981
/// (which only scans `spec.canonical`, not `spec.aliases`). At runtime,
/// `find_spec` returns the *first* matching spec for an aliased lookup,
/// so `-x` silently binds to whichever spec was registered first. This is
/// a fail-silent class — the user sets `debug` and gets `verbose`.
///
/// Not boilerplate: `duplicate_spec_errors` covers identical canonical
/// names (`verbose|v` + `verbose=s`) — that test passes because both
/// canonicals are literally "verbose". This test is the orthogonal case:
/// different canonicals, COLLIDING aliases. There is no existing test for
/// that path. A future fix (extending the dup-check to aliases) would
/// flip this from "Ok with verbose=1, debug=0" to "Err". Either is fine —
/// the pin catches the change.
#[test]
fn duplicate_alias_across_specs_silently_resolves_to_first_spec() {
    let mut interp = VMHelper::new();
    let argv = argv_ref(&["-x"]);
    // Both specs claim short alias `x`. The dup-check only fires on
    // canonical names, which differ here (`verbose` vs `debug`).
    let specs = specs_ref(&["verbose|x", "debug|x"]);

    let out = builtin_getopts(&mut interp, &[argv, specs], 1)
        .expect("collision is silent — no error today");

    // First spec wins. `-x` => verbose=1, debug=0 (the seed default for
    // an unset Bool spec).
    assert_eq!(
        hget_int(&out, "verbose"),
        Some(1),
        "the FIRST spec (`verbose|x`) wins the alias lookup, so -x sets verbose"
    );
    assert_eq!(
        hget_int(&out, "debug"),
        Some(0),
        "the SECOND spec (`debug|x`) is silently shadowed — debug stays at its \
         seed default of 0 even though the user typed `-x`"
    );
}

/// `=s%` hash spec accepts an empty key. `-D =val` flows through
/// `split_hash_kv` (strykelang/getopts.rs:292-300), which searches for the
/// FIRST `=` byte. For `"=val"` that's index 0, yielding key="" and
/// value="val". No validation rejects the empty key, so the hash ends up
/// with `{ "" => "val" }`. Most CLI parsers reject this — it's almost
/// always a typo (`-D=val` vs `-Dk=val`). The other half of the same
/// helper — values containing `=` — is also pinned here: `-D k=a=b`
/// stores `{ "k" => "a=b" }`, which IS the intended split-on-first-`=`
/// behavior, providing a control against accidentally tightening the
/// helper.
///
/// Not boilerplate: the existing `hash_kv` test only covers well-formed
/// `k1=v1` / `k2=v2` pairs. Neither the empty-key nor the value-with-`=`
/// path has an existing assertion. A future fix that rejects empty keys
/// (or that splits on a different `=`) would flip exactly one of these
/// assertions, surfacing the change.
#[test]
fn hash_kv_empty_key_is_accepted_and_value_with_equals_keeps_remainder() {
    let mut interp = VMHelper::new();
    let argv = argv_ref(&["-D", "=val", "-D", "k=a=b"]);
    let specs = specs_ref(&["define|D=s%"]);

    let out = builtin_getopts(&mut interp, &[argv, specs], 1).expect("hash kv parse");

    let define = out
        .as_hash_ref()
        .expect("result is a hash ref")
        .read()
        .get("define")
        .cloned()
        .expect("define key present");
    let inner: IndexMap<String, StrykeValue> = define
        .as_hash_ref()
        .expect("define is a hash ref")
        .read()
        .clone();

    // Bug-class pin: empty key was accepted instead of erroring.
    let v_empty = inner
        .get("")
        .expect("empty-key entry exists — `-D =val` stored under \"\" key");
    assert_eq!(
        v_empty.to_string(),
        "val",
        "empty-key entry should hold the value `val`"
    );

    // Control: split-on-first-`=` for value-with-`=`. This is the
    // intended behavior — pinning it guards the helper against a
    // "fix" that incorrectly tightens both branches at once.
    let v_eq = inner.get("k").expect("key `k` from `-D k=a=b` is present");
    assert_eq!(
        v_eq.to_string(),
        "a=b",
        "split_hash_kv must split on the FIRST `=` — value keeps the second `=`"
    );
}
