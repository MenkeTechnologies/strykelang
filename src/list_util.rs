//! Perl 5 `List::Util` — core Perl ships an XS `List/Util.pm`; stryke registers native Rust
//! implementations here so every `EXPORT_OK` name is callable and matches common Perl 5 semantics.

use std::sync::Arc;

use parking_lot::RwLock;
use rand::seq::SliceRandom;
use rand::Rng;

use crate::ast::{Block, Program};
use crate::interpreter::{ExecResult, Interpreter, ModuleExportLists, WantarrayCtx};
use crate::value::{BlessedRef, HeapObject, PerlSub, PerlValue};

/// True if the program may reference `List::Util` (`use`, `require`, or qualified calls).
/// Used to skip installing [`install_list_util`] for tiny programs (benchmark startup).
pub fn program_needs_list_util(program: &Program) -> bool {
    let s = format!("{program:?}");
    s.contains("List::Util")
        || s.contains("chunked")
        || s.contains("windowed")
        || s.contains("fold")
        || s.contains("inject")
        || s.contains("find_all")
}

/// Ensure [`install_list_util`] ran (cheap `contains_key` after the first program prepare).
/// Deferred from [`Interpreter::new`] so tiny scripts pay less fixed startup.
pub fn ensure_list_util(interp: &mut Interpreter) {
    if interp.subs.contains_key("List::Util::sum") {
        return;
    }
    install_list_util(interp);
}

/// `Scalar::Util` — native stubs (vendor `Scalar/Util.pm` is a no-op package header).
/// `Sub::Util::set_subname` / `subname` — core XS in perl; [`Try::Tiny`] optional-depends on these.
/// No-op naming: return the coderef so try/catch stack traces work without renaming closures.
pub fn install_sub_util(interp: &mut Interpreter) {
    if interp.subs.contains_key("Sub::Util::set_subname") {
        return;
    }
    let empty: Block = vec![];
    let export_ok: Vec<String> = SUB_UTIL_NATIVE.iter().map(|s| (*s).to_string()).collect();
    interp.module_export_lists.insert(
        "Sub::Util".to_string(),
        ModuleExportLists {
            export: vec![],
            export_ok,
        },
    );
    for name in SUB_UTIL_NATIVE {
        let key = format!("Sub::Util::{}", name);
        interp.subs.insert(
            key.clone(),
            Arc::new(PerlSub {
                name: key,
                params: vec![],
                body: empty.clone(),
                prototype: None,
                closure_env: None,
                fib_like: None,
            }),
        );
    }
}

const SUB_UTIL_NATIVE: &[&str] = &["set_subname", "subname"];

pub fn install_scalar_util(interp: &mut Interpreter) {
    if interp.subs.contains_key("Scalar::Util::blessed") {
        return;
    }
    let empty: Block = vec![];
    let export_ok: Vec<String> = SCALAR_UTIL_NATIVE
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    interp.module_export_lists.insert(
        "Scalar::Util".to_string(),
        ModuleExportLists {
            export: vec![],
            export_ok,
        },
    );
    for name in SCALAR_UTIL_NATIVE {
        let key = format!("Scalar::Util::{}", name);
        interp.subs.insert(
            key.clone(),
            Arc::new(PerlSub {
                name: key,
                params: vec![],
                body: empty.clone(),
                prototype: None,
                closure_env: None,
                fib_like: None,
            }),
        );
    }
}

const SCALAR_UTIL_NATIVE: &[&str] = &[
    "blessed", "refaddr", "reftype", "weaken", "unweaken", "isweak",
];

/// Insert placeholder subs (empty body) and route calls through `native_dispatch`.
pub fn install_list_util(interp: &mut Interpreter) {
    let empty: Block = vec![];
    let export_ok: Vec<String> = LIST_UTIL_ROOT.iter().map(|s| (*s).to_string()).collect();
    interp.module_export_lists.insert(
        "List::Util".to_string(),
        ModuleExportLists {
            export: export_ok.clone(),
            export_ok,
        },
    );
    for name in LIST_UTIL_ROOT {
        let key = format!("List::Util::{}", name);
        interp.subs.insert(
            key.clone(),
            Arc::new(PerlSub {
                name: key,
                params: vec![],
                body: empty.clone(),
                prototype: None,
                closure_env: None,
                fib_like: None,
            }),
        );
    }
    for name in PAIR_METHODS {
        let key = format!("List::Util::_Pair::{}", name);
        interp.subs.insert(
            key.clone(),
            Arc::new(PerlSub {
                name: key,
                params: vec![],
                body: empty.clone(),
                prototype: None,
                closure_env: None,
                fib_like: None,
            }),
        );
    }
}

const LIST_UTIL_ROOT: &[&str] = &[
    "all",
    "any",
    "first",
    "min",
    "max",
    "minstr",
    "maxstr",
    "mean",
    "median",
    "mode",
    "none",
    "notall",
    "product",
    "reduce",
    "fold",
    "reductions",
    "sum",
    "sum0",
    "stddev",
    "variance",
    "sample",
    "shuffle",
    "uniq",
    "uniqint",
    "uniqnum",
    "uniqstr",
    "zip",
    "zip_longest",
    "zip_shortest",
    "mesh",
    "mesh_longest",
    "mesh_shortest",
    "chunked",
    "windowed",
    "head",
    "tail",
    "pairs",
    "unpairs",
    "pairkeys",
    "pairvalues",
    "pairmap",
    "pairgrep",
    "pairfirst",
];

const PAIR_METHODS: &[&str] = &["key", "value", "TO_JSON"];

/// If `sub` is a native `List::Util::*` stub, run the Rust implementation.
pub(crate) fn native_dispatch(
    interp: &mut Interpreter,
    sub: &PerlSub,
    args: &[PerlValue],
    want: WantarrayCtx,
) -> Option<ExecResult> {
    match sub.name.as_str() {
        "List::Util::uniq" => Some(dispatch_ok(uniq_with_want(args, want))),
        "List::Util::uniqstr" => Some(dispatch_ok(uniqstr_with_want(args, want))),
        "List::Util::uniqint" => Some(dispatch_ok(uniqint_with_want(args, want))),
        "List::Util::uniqnum" => Some(dispatch_ok(uniqnum_with_want(args, want))),
        "List::Util::sum" => Some(dispatch_ok(sum(args).map(|v| aggregate_wantarray(v, want)))),
        "List::Util::sum0" => Some(dispatch_ok(
            sum0(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::product" => Some(dispatch_ok(
            product(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::mean" => Some(dispatch_ok(
            mean(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::median" => Some(dispatch_ok(
            median(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::mode" => Some(dispatch_ok(mode_with_want(args, want))),
        "List::Util::variance" => Some(dispatch_ok(
            variance(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::stddev" => Some(dispatch_ok(
            stddev(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::min" => Some(dispatch_ok(
            minmax(args, MinMax::MinNum).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::max" => Some(dispatch_ok(
            minmax(args, MinMax::MaxNum).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::minstr" => Some(dispatch_ok(
            minmax(args, MinMax::MinStr).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::maxstr" => Some(dispatch_ok(
            minmax(args, MinMax::MaxStr).map(|v| aggregate_wantarray(v, want)),
        )),
        "List::Util::shuffle" => Some(dispatch_ok(shuffle_native(interp, args))),
        "List::Util::chunked" => Some(dispatch_ok(chunked_with_want(args, want))),
        "List::Util::windowed" => Some(dispatch_ok(windowed_with_want(args, want))),
        "List::Util::sample" => Some(dispatch_ok(sample_native(interp, args))),
        "List::Util::head" => Some(dispatch_ok(head_tail_take_impl(
            args,
            HeadTailTake::ListUtilHead,
            want,
        ))),
        "List::Util::tail" => Some(dispatch_ok(head_tail_take_impl(
            args,
            HeadTailTake::ListUtilTail,
            want,
        ))),
        "List::Util::reduce" | "List::Util::fold" => Some(reduce_like(interp, args, want, false)),
        "List::Util::reductions" => Some(reduce_like(interp, args, want, true)),
        "List::Util::any" => Some(any_all_none(interp, args, want, AnyMode::Any)),
        "List::Util::all" => Some(any_all_none(interp, args, want, AnyMode::All)),
        "List::Util::none" => Some(any_all_none(interp, args, want, AnyMode::None)),
        "List::Util::notall" => Some(any_all_none(interp, args, want, AnyMode::NotAll)),
        "List::Util::first" => Some(first_native(interp, args, want)),
        "List::Util::pairs" => Some(dispatch_ok(pairs_native(args))),
        "List::Util::unpairs" => Some(dispatch_ok(unpairs_native(args))),
        "List::Util::pairkeys" => Some(dispatch_ok(pairkeys_values(true, args))),
        "List::Util::pairvalues" => Some(dispatch_ok(pairkeys_values(false, args))),
        "List::Util::pairgrep" => Some(pairgrep_map(interp, args, want, PairMode::Grep)),
        "List::Util::pairmap" => Some(pairgrep_map(interp, args, want, PairMode::Map)),
        "List::Util::pairfirst" => Some(pairgrep_map(interp, args, want, PairMode::First)),
        "List::Util::zip" | "List::Util::zip_longest" => {
            Some(dispatch_ok(zip_mesh(args, ZipMesh::ZipLongest)))
        }
        "List::Util::zip_shortest" => Some(dispatch_ok(zip_mesh(args, ZipMesh::ZipShortest))),
        "List::Util::mesh" | "List::Util::mesh_longest" => {
            Some(dispatch_ok(zip_mesh(args, ZipMesh::MeshLongest)))
        }
        "List::Util::mesh_shortest" => Some(dispatch_ok(zip_mesh(args, ZipMesh::MeshShortest))),
        "List::Util::_Pair::key" => Some(dispatch_ok(pair_accessor(args, 0))),
        "List::Util::_Pair::value" => Some(dispatch_ok(pair_accessor(args, 1))),
        "List::Util::_Pair::TO_JSON" => Some(dispatch_ok(pair_to_json(args))),
        "Scalar::Util::blessed" => Some(dispatch_ok(scalar_util_blessed(args.first()))),
        "Scalar::Util::refaddr" => Some(dispatch_ok(scalar_util_refaddr(args.first()))),
        "Scalar::Util::reftype" => Some(dispatch_ok(scalar_util_reftype(args.first()))),
        "Scalar::Util::weaken" | "Scalar::Util::unweaken" => {
            Some(dispatch_ok(Ok(PerlValue::UNDEF)))
        }
        "Scalar::Util::isweak" => Some(dispatch_ok(Ok(PerlValue::integer(0)))),
        "Sub::Util::set_subname" | "Sub::Util::subname" => {
            Some(dispatch_ok(sub_util_set_subname(args)))
        }
        // Core XS in perl; JSON::PP BEGIN uses this before utf8_heavy loads (see utf8::AUTOLOAD).
        "utf8::unicode_to_native" => Some(dispatch_ok(utf8_unicode_to_native(args.first()))),
        _ => None,
    }
}

/// Perl: `set_subname $name, $coderef` → returns `$coderef` (stryke does not rename closures).
fn sub_util_set_subname(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    Ok(args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
}

fn utf8_unicode_to_native(arg: Option<&PerlValue>) -> crate::error::PerlResult<PerlValue> {
    let n = arg.map(|a| a.to_int()).unwrap_or(0);
    Ok(PerlValue::integer(n))
}

fn scalar_util_blessed(arg: Option<&PerlValue>) -> crate::error::PerlResult<PerlValue> {
    let Some(v) = arg else {
        return Ok(PerlValue::UNDEF);
    };
    Ok(v.as_blessed_ref()
        .map(|b| PerlValue::string(b.class.clone()))
        .unwrap_or(PerlValue::UNDEF))
}

fn scalar_util_refaddr(arg: Option<&PerlValue>) -> crate::error::PerlResult<PerlValue> {
    let Some(v) = arg else {
        return Ok(PerlValue::UNDEF);
    };
    if v.is_undef() {
        return Ok(PerlValue::UNDEF);
    }
    if v.with_heap(|_| ()).is_none() {
        return Ok(PerlValue::UNDEF);
    }
    Ok(PerlValue::integer(v.raw_bits() as i64))
}

fn scalar_util_reftype(arg: Option<&PerlValue>) -> crate::error::PerlResult<PerlValue> {
    let Some(v) = arg else {
        return Ok(PerlValue::UNDEF);
    };
    if v.is_undef() {
        return Ok(PerlValue::UNDEF);
    }
    if let Some(b) = v.as_blessed_ref() {
        let inner = b.data.read().clone();
        return scalar_util_reftype(Some(&inner));
    }
    Ok(v.with_heap(|h| {
        let t = match h {
            HeapObject::Array(_) | HeapObject::ArrayRef(_) | HeapObject::ArrayBindingRef(_) => {
                Some("ARRAY")
            }
            HeapObject::Hash(_) | HeapObject::HashRef(_) | HeapObject::HashBindingRef(_) => {
                Some("HASH")
            }
            HeapObject::ScalarRef(_) | HeapObject::ScalarBindingRef(_) => Some("SCALAR"),
            HeapObject::CodeRef(_) => Some("CODE"),
            HeapObject::Regex(_, _, _) => Some("REGEXP"),
            _ => None,
        };
        t.map(|s| PerlValue::string(s.to_string()))
    })
    .flatten()
    .unwrap_or(PerlValue::UNDEF))
}

fn dispatch_ok(r: crate::error::PerlResult<PerlValue>) -> ExecResult {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(e.into()),
    }
}

/// Perl list context for these subs is a return **list** of one scalar (possibly `undef`).
#[inline]
fn aggregate_wantarray(v: PerlValue, want: WantarrayCtx) -> PerlValue {
    if want == WantarrayCtx::List {
        PerlValue::array(vec![v])
    } else {
        v
    }
}

enum MinMax {
    MinNum,
    MaxNum,
    MinStr,
    MaxStr,
}

fn minmax(args: &[PerlValue], mode: MinMax) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let mut it = args.iter().cloned();
    let mut m = it.next().unwrap();
    for x in it {
        m = match mode {
            MinMax::MinNum => {
                if x.to_number() < m.to_number() {
                    x
                } else {
                    m
                }
            }
            MinMax::MaxNum => {
                if x.to_number() > m.to_number() {
                    x
                } else {
                    m
                }
            }
            MinMax::MinStr => {
                if x.to_string().cmp(&m.to_string()) == std::cmp::Ordering::Less {
                    x
                } else {
                    m
                }
            }
            MinMax::MaxStr => {
                if x.to_string().cmp(&m.to_string()) == std::cmp::Ordering::Greater {
                    x
                } else {
                    m
                }
            }
        };
    }
    Ok(m)
}

fn uniq_with_want(args: &[PerlValue], want: WantarrayCtx) -> crate::error::PerlResult<PerlValue> {
    let a = uniq_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(PerlValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

/// Adjacent-unique like Perl 5 `uniq` (DWIM string/undef; refs compared by string form).
fn uniq_list(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for arg in args {
        if arg.is_iterator() {
            let iter = arg.clone().into_iterator();
            while let Some(x) = iter.next_item() {
                let key = x.to_string();
                if seen.insert(key) {
                    out.push(x);
                }
            }
        } else if let Some(arr) = arg.as_array_vec() {
            for x in arr {
                let key = x.to_string();
                if seen.insert(key) {
                    out.push(x.clone());
                }
            }
        } else {
            let key = arg.to_string();
            if seen.insert(key) {
                out.push(arg.clone());
            }
        }
    }
    Ok(PerlValue::array(out))
}

fn uniqstr_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    let a = uniqstr_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(PerlValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

fn uniqstr_list(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut prev: Option<String> = None;
    let mut have = false;
    for x in args.iter().cloned() {
        let s = x.to_string();
        if !have || prev.as_ref() != Some(&s) {
            out.push(x);
            prev = Some(s);
            have = true;
        }
    }
    Ok(PerlValue::array(out))
}

fn uniqint_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    let a = uniqint_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(PerlValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

fn uniqint_list(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut prev: Option<i64> = None;
    let mut have = false;
    for x in args {
        let n = x.to_int();
        if !have || prev != Some(n) {
            out.push(PerlValue::integer(n));
            prev = Some(n);
            have = true;
        }
    }
    Ok(PerlValue::array(out))
}

fn num_eq(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    a == b
}

fn uniqnum_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    let a = uniqnum_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(PerlValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

fn uniqnum_list(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut prev: Option<f64> = None;
    let mut have = false;
    for x in args.iter().cloned() {
        let n = x.to_number();
        if !have || !num_eq(prev.unwrap(), n) {
            out.push(x);
            prev = Some(n);
            have = true;
        }
    }
    Ok(PerlValue::array(out))
}

fn sum(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let mut s = 0.0;
    for x in args {
        if x.is_iterator() {
            let iter = x.clone().into_iterator();
            while let Some(item) = iter.next_item() {
                s += item.to_number();
            }
        } else if let Some(arr) = x.as_array_vec() {
            for item in arr {
                s += item.to_number();
            }
        } else {
            s += x.to_number();
        }
    }
    Ok(PerlValue::float(s))
}

fn sum0(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut s = 0.0;
    for x in args {
        if x.is_iterator() {
            let iter = x.clone().into_iterator();
            while let Some(item) = iter.next_item() {
                s += item.to_number();
            }
        } else if let Some(arr) = x.as_array_vec() {
            for item in arr {
                s += item.to_number();
            }
        } else {
            s += x.to_number();
        }
    }
    Ok(PerlValue::float(s))
}

fn product(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut p = 1.0;
    for x in args {
        if x.is_iterator() {
            let iter = x.clone().into_iterator();
            while let Some(item) = iter.next_item() {
                p *= item.to_number();
            }
        } else if let Some(arr) = x.as_array_vec() {
            for item in arr {
                p *= item.to_number();
            }
        } else {
            p *= x.to_number();
        }
    }
    Ok(PerlValue::float(p))
}

/// Arithmetic mean; empty list → `undef`.
fn mean(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let n = args.len() as f64;
    let s: f64 = args.iter().map(|x| x.to_number()).sum();
    Ok(PerlValue::float(s / n))
}

/// Median (linear interpolation for even length). Empty list → `undef`.
fn median(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let mut v: Vec<f64> = args.iter().map(|x| x.to_number()).collect();
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let mid = if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    };
    Ok(PerlValue::float(mid))
}

/// Values with highest frequency (ties all returned in list context). Empty list → `undef` / empty list.
fn mode_with_want(args: &[PerlValue], want: WantarrayCtx) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::List => PerlValue::array(vec![]),
            WantarrayCtx::Scalar | WantarrayCtx::Void => PerlValue::UNDEF,
        });
    }
    let nums: Vec<f64> = args.iter().map(|x| x.to_number()).collect();
    let mut idx: Vec<usize> = (0..args.len()).collect();
    idx.sort_by(|&i, &j| nums[i].total_cmp(&nums[j]));
    let mut best_len = 0usize;
    let mut mode_starts: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < idx.len() {
        let mut j = i + 1;
        while j < idx.len() && num_eq(nums[idx[i]], nums[idx[j]]) {
            j += 1;
        }
        let run_len = j - i;
        if run_len > best_len {
            best_len = run_len;
            mode_starts.clear();
            mode_starts.push(idx[i]);
        } else if run_len == best_len {
            mode_starts.push(idx[i]);
        }
        i = j;
    }
    let modes: Vec<PerlValue> = mode_starts.into_iter().map(|ix| args[ix].clone()).collect();
    let first = modes.first().cloned().unwrap_or(PerlValue::UNDEF);
    Ok(match want {
        WantarrayCtx::List => PerlValue::array(modes),
        WantarrayCtx::Scalar => first,
        WantarrayCtx::Void => PerlValue::UNDEF,
    })
}

/// Population variance (divide by N). Empty → `undef`; one element → `0`.
fn variance(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let n = args.len() as f64;
    let mean_v: f64 = args.iter().map(|x| x.to_number()).sum::<f64>() / n;
    let var: f64 = args
        .iter()
        .map(|x| {
            let d = x.to_number() - mean_v;
            d * d
        })
        .sum::<f64>()
        / n;
    Ok(PerlValue::float(var))
}

fn stddev(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let var = variance(args)?;
    Ok(PerlValue::float(var.to_number().sqrt()))
}

fn shuffle_native(
    interp: &mut Interpreter,
    args: &[PerlValue],
) -> crate::error::PerlResult<PerlValue> {
    let mut v: Vec<PerlValue> = args.to_vec();
    v.shuffle(&mut interp.rand_rng);
    Ok(PerlValue::array(v))
}

/// `chunked LIST, N` — last argument is chunk size; preceding values are the list. Returns a list of
/// arrayrefs (same shape as `zip` rows). Scalar context: number of chunks.
fn chunked_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(crate::error::PerlError::runtime(
            "List::Util::chunked: expected LIST, N",
            0,
        ));
    }
    // Last arg is always the chunk size N; everything before it is the list.
    // `chunked(3)` → N=3, empty list.  `chunked(@list, 2)` → N=2, list items.
    let n = args[args.len() - 1].to_int().max(0) as usize;
    let items: Vec<PerlValue> = args[..args.len().saturating_sub(1)].to_vec();
    if n == 0 {
        return Ok(match want {
            WantarrayCtx::Scalar => PerlValue::integer(0),
            _ => PerlValue::array(vec![]),
        });
    }
    let mut chunk_refs = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let end = (i + n).min(items.len());
        chunk_refs.push(PerlValue::array_ref(Arc::new(RwLock::new(
            items[i..end].to_vec(),
        ))));
        i = end;
    }
    let n_chunks = chunk_refs.len() as i64;
    let out = PerlValue::array(chunk_refs);
    Ok(match want {
        WantarrayCtx::Scalar => PerlValue::integer(n_chunks),
        _ => out,
    })
}

/// `windowed LIST, N` — last argument is window size; preceding values are the list. Overlapping
/// sliding windows (step 1), each window an arrayref like [`chunked_with_want`]. No partial trailing
/// windows. Scalar context: window count.
fn windowed_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(crate::error::PerlError::runtime(
            "List::Util::windowed: expected LIST, N",
            0,
        ));
    }
    // windowed @l == windowed @l, 2 — single arg is the list, window size defaults to 2
    let (n, items) = if args.len() == 1 {
        (2usize, args[0].to_list())
    } else {
        let n = args[args.len() - 1].to_int().max(0) as usize;
        let items: Vec<PerlValue> = args[..args.len().saturating_sub(1)].to_vec();
        (n, items)
    };
    if n == 0 || items.len() < n {
        return Ok(match want {
            WantarrayCtx::Scalar => PerlValue::integer(0),
            _ => PerlValue::array(vec![]),
        });
    }
    let mut windows = Vec::new();
    for i in 0..=(items.len() - n) {
        windows.push(PerlValue::array_ref(Arc::new(RwLock::new(
            items[i..i + n].to_vec(),
        ))));
    }
    let nw = windows.len() as i64;
    let out = PerlValue::array(windows);
    Ok(match want {
        WantarrayCtx::Scalar => PerlValue::integer(nw),
        _ => out,
    })
}

fn sample_native(
    interp: &mut Interpreter,
    args: &[PerlValue],
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let n = args[0].to_int().max(0) as usize;
    let mut pool: Vec<PerlValue> = args[1..].to_vec();
    let mut out = Vec::new();
    for _ in 0..n {
        if pool.is_empty() {
            break;
        }
        let j = interp.rand_rng.gen_range(0..pool.len());
        out.push(pool.swap_remove(j));
    }
    Ok(PerlValue::array(out))
}

#[derive(Clone, Copy)]
pub(crate) enum HeadTailTake {
    /// Builtin `take` / bare `head` — negative count is treated as zero (`max(0)`).
    Take,
    /// `List::Util::head` — negative count means “all but last |k|”.
    ListUtilHead,
    /// `List::Util::tail` — same size rules as Perl `tail`.
    ListUtilTail,
}

/// Shared by [`crate::builtins::builtin_take`], bare `head`, and `List::Util::head` / `tail`.
/// **Argument order:** list operands first, **count last** — `take(@l, N)`, `List::Util::head(10,20,30,2)`.
/// A single argument is treated as **N** with an empty list (`take(3)` → empty).
/// List context: array slice; scalar context: last element of that slice, or `undef` if empty.
pub(crate) fn head_tail_take_impl(
    args: &[PerlValue],
    kind: HeadTailTake,
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::Scalar => PerlValue::UNDEF,
            _ => PerlValue::array(vec![]),
        });
    }
    let (raw, list) = if args.len() == 1 {
        // head @l == head @l, 1 — single arg is the list, count defaults to 1
        let mut list = Vec::new();
        list.extend(args[0].to_list());
        (1, list)
    } else {
        // Count is always the last argument: `take(@list, N)` / `@list |> take N`
        let raw = args[args.len() - 1].to_int();
        let mut list = Vec::new();
        for a in &args[..args.len() - 1] {
            list.extend(a.to_list());
        }
        (raw, list)
    };
    let n = list.len() as i64;
    let take_n = match kind {
        HeadTailTake::Take => {
            let size = raw.max(0);
            size.min(n).max(0) as usize
        }
        HeadTailTake::ListUtilHead | HeadTailTake::ListUtilTail => {
            let size = raw;
            if size >= 0 {
                size.min(n).max(0) as usize
            } else {
                let k = (-size).min(n);
                (n - k) as usize
            }
        }
    };
    let out: Vec<PerlValue> = match kind {
        HeadTailTake::Take | HeadTailTake::ListUtilHead => list.into_iter().take(take_n).collect(),
        HeadTailTake::ListUtilTail => {
            let len = list.len();
            let skip = len.saturating_sub(take_n);
            list.into_iter().skip(skip).collect()
        }
    };
    Ok(match want {
        WantarrayCtx::Scalar => out.last().cloned().unwrap_or(PerlValue::UNDEF),
        _ => PerlValue::array(out),
    })
}

/// Builtin `tail` — last `$n` items; negative `$n` clamps to zero (empty). Operands are
/// **list values then count**: `tail(@l, N)`. One argument is the list with count defaulting to 1.
/// When the list is a single string containing newlines, split into lines first (Rust [`str::lines`] rules).
pub(crate) fn extension_tail_impl(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::Scalar => PerlValue::UNDEF,
            _ => PerlValue::array(vec![]),
        });
    }
    // tail @l == tail @l, 1 — single arg is the list, count defaults to 1
    let raw = if args.len() == 1 {
        1
    } else {
        args[args.len() - 1].to_int()
    };
    let mut list: Vec<PerlValue> = if args.len() == 1 {
        args[0].to_list()
    } else {
        let mut list = Vec::new();
        for a in &args[..args.len() - 1] {
            list.extend(a.to_list());
        }
        list
    };
    if list.len() == 1 && list[0].is_string_like() {
        let s = list[0].to_string();
        if s.contains('\n') || s.contains('\r') {
            list = s
                .lines()
                .map(|ln| PerlValue::string(ln.to_string()))
                .collect();
        }
    }
    let n = list.len() as i64;
    let take_n = raw.max(0).min(n).max(0) as usize;
    let len = list.len();
    let skip = len.saturating_sub(take_n);
    let out: Vec<PerlValue> = list.into_iter().skip(skip).collect();
    Ok(match want {
        WantarrayCtx::Scalar => out.last().cloned().unwrap_or(PerlValue::UNDEF),
        _ => PerlValue::array(out),
    })
}

/// Builtin `drop` — skip the first `$n` items; negative `$n` clamps to zero. Operands are
/// **list values then count**: `drop(@l, N)`. One argument is the list with count defaulting to 1.
/// Same multiline-string line split as [`extension_tail_impl`].
pub(crate) fn extension_drop_impl(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::Scalar => PerlValue::UNDEF,
            _ => PerlValue::array(vec![]),
        });
    }
    // drop @l == drop @l, 1 — single arg is the list, count defaults to 1
    let raw = if args.len() == 1 {
        1
    } else {
        args[args.len() - 1].to_int()
    };
    let mut list: Vec<PerlValue> = if args.len() == 1 {
        args[0].to_list()
    } else {
        let mut list = Vec::new();
        for a in &args[..args.len() - 1] {
            list.extend(a.to_list());
        }
        list
    };
    if list.len() == 1 && list[0].is_string_like() {
        let s = list[0].to_string();
        if s.contains('\n') || s.contains('\r') {
            list = s
                .lines()
                .map(|ln| PerlValue::string(ln.to_string()))
                .collect();
        }
    }
    let n = list.len();
    let skip_n = raw.max(0).min(n as i64) as usize;
    let out: Vec<PerlValue> = list.into_iter().skip(skip_n).collect();
    Ok(match want {
        WantarrayCtx::Scalar => out.last().cloned().unwrap_or(PerlValue::UNDEF),
        _ => PerlValue::array(out),
    })
}

fn reduce_like(
    interp: &mut Interpreter,
    args: &[PerlValue],
    want: WantarrayCtx,
    reductions: bool,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::PerlError::runtime(
                "List::Util::reduce: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<PerlValue> = args[1..].to_vec();
    if items.is_empty() {
        if reductions {
            return Ok(PerlValue::array(vec![]));
        }
        return Ok(PerlValue::UNDEF);
    }
    if items.len() == 1 {
        if reductions {
            return Ok(PerlValue::array(vec![items[0].clone()]));
        }
        return Ok(items[0].clone());
    }
    let mut acc = items[0].clone();
    let mut chain: Vec<PerlValue> = if reductions {
        vec![acc.clone()]
    } else {
        vec![]
    };
    for b in items.iter().skip(1) {
        let _ = interp.scope.set_scalar("a", acc.clone());
        let _ = interp.scope.set_scalar("b", b.clone());
        let _ = interp.scope.set_scalar("_0", acc.clone());
        let _ = interp.scope.set_scalar("_1", b.clone());
        acc = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        if reductions {
            chain.push(acc.clone());
        }
    }
    if reductions {
        if want == WantarrayCtx::Scalar {
            return Ok(chain.last().cloned().unwrap_or(PerlValue::UNDEF));
        }
        return Ok(PerlValue::array(chain));
    }
    Ok(acc)
}

enum AnyMode {
    Any,
    All,
    None,
    NotAll,
}

fn any_all_none(
    interp: &mut Interpreter,
    args: &[PerlValue],
    _want: WantarrayCtx,
    mode: AnyMode,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::PerlError::runtime(
                "List::Util::any/all/...: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<PerlValue> = args[1..].to_vec();
    let empty_ok = matches!(mode, AnyMode::All | AnyMode::None);
    if items.is_empty() {
        return Ok(PerlValue::integer(if empty_ok { 1 } else { 0 }));
    }
    for it in items {
        interp.scope.set_topic(it);
        let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        let t = v.is_true();
        match mode {
            AnyMode::Any if t => return Ok(PerlValue::integer(1)),
            AnyMode::All if !t => return Ok(PerlValue::integer(0)),
            AnyMode::None if t => return Ok(PerlValue::integer(0)),
            AnyMode::NotAll if !t => return Ok(PerlValue::integer(1)),
            _ => {}
        }
    }
    Ok(PerlValue::integer(match mode {
        AnyMode::Any => 0,
        AnyMode::All => 1,
        AnyMode::None => 1,
        AnyMode::NotAll => 0,
    }))
}

fn first_native(interp: &mut Interpreter, args: &[PerlValue], _want: WantarrayCtx) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::PerlError::runtime(
                "List::Util::first: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<PerlValue> = args[1..].to_vec();
    for it in items {
        interp.scope.set_topic(it.clone());
        let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        if v.is_true() {
            return Ok(it);
        }
    }
    Ok(PerlValue::UNDEF)
}

fn pairs_native(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        let row = vec![args[i].clone(), args[i + 1].clone()];
        let ar = PerlValue::array_ref(Arc::new(RwLock::new(row)));
        let b = PerlValue::blessed(Arc::new(BlessedRef::new_blessed(
            "List::Util::_Pair".to_string(),
            ar,
        )));
        out.push(b);
        i += 2;
    }
    Ok(PerlValue::array(out))
}

fn unpairs_native(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    for x in args {
        if let Some(r) = x.as_array_ref() {
            let g = r.read();
            out.push(g.first().cloned().unwrap_or(PerlValue::UNDEF));
            out.push(g.get(1).cloned().unwrap_or(PerlValue::UNDEF));
        } else if let Some(b) = x.as_blessed_ref() {
            if b.class == "List::Util::_Pair" {
                let d = b.data.read();
                if let Some(r) = d.as_array_ref() {
                    let g = r.read();
                    out.push(g.first().cloned().unwrap_or(PerlValue::UNDEF));
                    out.push(g.get(1).cloned().unwrap_or(PerlValue::UNDEF));
                }
            } else {
                out.push(PerlValue::UNDEF);
                out.push(PerlValue::UNDEF);
            }
        } else {
            out.push(PerlValue::UNDEF);
            out.push(PerlValue::UNDEF);
        }
    }
    Ok(PerlValue::array(out))
}

fn pairkeys_values(keys: bool, args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.push(if keys {
            args[i].clone()
        } else {
            args[i + 1].clone()
        });
        i += 2;
    }
    Ok(PerlValue::array(out))
}

enum PairMode {
    Grep,
    Map,
    First,
}

fn pairgrep_map(
    interp: &mut Interpreter,
    args: &[PerlValue],
    want: WantarrayCtx,
    mode: PairMode,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::PerlError::runtime(
                "pairgrep/pairmap/pairfirst: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let flat: Vec<PerlValue> = args[1..].to_vec();
    match mode {
        PairMode::Grep => {
            let mut out = Vec::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                let a = flat[i].clone();
                let b = flat[i + 1].clone();
                let _ = interp.scope.set_scalar("a", a.clone());
                let _ = interp.scope.set_scalar("b", b.clone());
                let _ = interp.scope.set_scalar("_0", a.clone());
                let _ = interp.scope.set_scalar("_1", b.clone());
                let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
                if v.is_true() {
                    out.push(a);
                    out.push(b);
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(PerlValue::integer((out.len() / 2) as i64));
            }
            Ok(PerlValue::array(out))
        }
        PairMode::Map => {
            let mut out = Vec::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                let _ = interp.scope.set_scalar("a", flat[i].clone());
                let _ = interp.scope.set_scalar("b", flat[i + 1].clone());
                let _ = interp.scope.set_scalar("_0", flat[i].clone());
                let _ = interp.scope.set_scalar("_1", flat[i + 1].clone());
                let produced = interp.call_sub(&code, vec![], WantarrayCtx::List, 0)?;
                if let Some(items) = produced.as_array_vec() {
                    out.extend(items);
                } else {
                    out.push(produced);
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(PerlValue::integer(out.len() as i64));
            }
            Ok(PerlValue::array(out))
        }
        PairMode::First => {
            let mut i = 0;
            while i + 1 < flat.len() {
                let a = flat[i].clone();
                let b = flat[i + 1].clone();
                let _ = interp.scope.set_scalar("a", a.clone());
                let _ = interp.scope.set_scalar("b", b.clone());
                let _ = interp.scope.set_scalar("_0", a.clone());
                let _ = interp.scope.set_scalar("_1", b.clone());
                let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
                if v.is_true() {
                    if want == WantarrayCtx::Scalar {
                        return Ok(PerlValue::integer(1));
                    }
                    return Ok(PerlValue::array(vec![a, b]));
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(PerlValue::integer(0));
            }
            Ok(PerlValue::array(vec![]))
        }
    }
}

fn pair_accessor(args: &[PerlValue], idx: usize) -> crate::error::PerlResult<PerlValue> {
    let obj = args.first().ok_or_else(|| {
        crate::error::PerlError::runtime("List::Util::_Pair::key/value: missing invocant", 0)
    })?;
    pair_field(obj, idx)
}

fn pair_field(obj: &PerlValue, idx: usize) -> crate::error::PerlResult<PerlValue> {
    let b = obj.as_blessed_ref().ok_or_else(|| {
        crate::error::PerlError::runtime("List::Util::_Pair::method: not a pair object", 0)
    })?;
    if b.class != "List::Util::_Pair" {
        return Err(crate::error::PerlError::runtime(
            "List::Util::_Pair::method: not a pair object",
            0,
        ));
    }
    let d = b.data.read();
    if let Some(r) = d.as_array_ref() {
        let g = r.read();
        return Ok(g.get(idx).cloned().unwrap_or(PerlValue::UNDEF));
    }
    Err(crate::error::PerlError::runtime(
        "List::Util::_Pair: internal data is not an ARRAY reference",
        0,
    ))
}

fn pair_to_json(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let obj = args.first().ok_or_else(|| {
        crate::error::PerlError::runtime("List::Util::_Pair::TO_JSON: missing invocant", 0)
    })?;
    let k = pair_field(obj, 0)?;
    let v = pair_field(obj, 1)?;
    Ok(PerlValue::array(vec![k, v]))
}

enum ZipMesh {
    ZipLongest,
    ZipShortest,
    MeshLongest,
    MeshShortest,
}

fn zip_mesh(args: &[PerlValue], mode: ZipMesh) -> crate::error::PerlResult<PerlValue> {
    let arrays: Vec<Vec<PerlValue>> = args.iter().map(arg_to_list).collect();
    if arrays.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let min_len = arrays.iter().map(|a| a.len()).min().unwrap_or(0);
    let max_len = arrays.iter().map(|a| a.len()).max().unwrap_or(0);
    let len = match mode {
        ZipMesh::ZipShortest | ZipMesh::MeshShortest => min_len,
        ZipMesh::ZipLongest | ZipMesh::MeshLongest => max_len,
    };
    match mode {
        ZipMesh::ZipLongest | ZipMesh::ZipShortest => {
            let mut out = Vec::new();
            for i in 0..len {
                let mut row = Vec::new();
                for a in &arrays {
                    row.push(a.get(i).cloned().unwrap_or(PerlValue::UNDEF));
                }
                out.push(PerlValue::array_ref(Arc::new(RwLock::new(row))));
            }
            Ok(PerlValue::array(out))
        }
        ZipMesh::MeshLongest | ZipMesh::MeshShortest => {
            let mut out = Vec::new();
            for i in 0..len {
                for a in &arrays {
                    out.push(a.get(i).cloned().unwrap_or(PerlValue::UNDEF));
                }
            }
            Ok(PerlValue::array(out))
        }
    }
}

fn arg_to_list(v: &PerlValue) -> Vec<PerlValue> {
    if let Some(a) = v.as_array_vec() {
        a
    } else if let Some(r) = v.as_array_ref() {
        r.read().clone()
    } else {
        vec![v.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::{Interpreter, WantarrayCtx};
    use crate::value::PerlValue;

    fn call_native(
        interp: &mut Interpreter,
        fq: &str,
        args: &[PerlValue],
        want: WantarrayCtx,
    ) -> PerlValue {
        ensure_list_util(interp);
        let sub = interp
            .subs
            .get(fq)
            .unwrap_or_else(|| panic!("missing sub {fq}"))
            .clone();
        match native_dispatch(interp, &sub, args, want) {
            Some(Ok(v)) => v,
            Some(Err(e)) => panic!("{:?}", e),
            None => panic!("not a List::Util native: {fq}"),
        }
    }

    #[test]
    fn sum_and_product() {
        let mut i = Interpreter::new();
        let s = call_native(
            &mut i,
            "List::Util::sum",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(3),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(s.to_int(), 6);
        let p = call_native(
            &mut i,
            "List::Util::product",
            &[PerlValue::integer(2), PerlValue::integer(3)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(p.to_int(), 6);
    }

    #[test]
    fn sum_empty_is_undef_sum0_empty_is_zero() {
        let mut i = Interpreter::new();
        let s = call_native(&mut i, "List::Util::sum", &[], WantarrayCtx::Scalar);
        assert!(s.is_undef());
        let z = call_native(&mut i, "List::Util::sum0", &[], WantarrayCtx::Scalar);
        assert_eq!(z.to_int(), 0);
    }

    #[test]
    fn product_empty_is_one() {
        let mut i = Interpreter::new();
        let p = call_native(&mut i, "List::Util::product", &[], WantarrayCtx::Scalar);
        assert_eq!(p.to_int(), 1);
    }

    #[test]
    fn min_max_minstr_maxstr() {
        let mut i = Interpreter::new();
        let mn = call_native(
            &mut i,
            "List::Util::min",
            &[PerlValue::float(3.0), PerlValue::float(1.0)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(mn.to_int(), 1);
        let mx = call_native(
            &mut i,
            "List::Util::max",
            &[PerlValue::integer(3), PerlValue::integer(9)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(mx.to_int(), 9);
        let ms = call_native(
            &mut i,
            "List::Util::minstr",
            &[PerlValue::string("z".into()), PerlValue::string("a".into())],
            WantarrayCtx::Scalar,
        );
        assert_eq!(ms.to_string(), "a");
    }

    #[test]
    fn mean_median_mode_variance_stddev() {
        let mut i = Interpreter::new();
        assert!(call_native(&mut i, "List::Util::mean", &[], WantarrayCtx::Scalar).is_undef());
        let m = call_native(
            &mut i,
            "List::Util::mean",
            &[
                PerlValue::integer(2),
                PerlValue::integer(4),
                PerlValue::integer(10),
            ],
            WantarrayCtx::Scalar,
        );
        assert!((m.to_number() - 16.0 / 3.0).abs() < 1e-9);

        let med_odd = call_native(
            &mut i,
            "List::Util::median",
            &[
                PerlValue::integer(3),
                PerlValue::integer(1),
                PerlValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(med_odd.to_int(), 2);

        let med_even = call_native(
            &mut i,
            "List::Util::median",
            &[
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
                PerlValue::integer(40),
            ],
            WantarrayCtx::Scalar,
        );
        assert!((med_even.to_number() - 25.0).abs() < 1e-9);

        let mode_sc = call_native(
            &mut i,
            "List::Util::mode",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(2),
                PerlValue::integer(3),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(mode_sc.to_int(), 2);

        let mode_li = call_native(
            &mut i,
            "List::Util::mode",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(2),
                PerlValue::integer(3),
                PerlValue::integer(3),
            ],
            WantarrayCtx::List,
        );
        let mv = mode_li.as_array_vec().expect("mode list");
        assert_eq!(mv.len(), 2);
        assert_eq!(mv[0].to_int(), 2);
        assert_eq!(mv[1].to_int(), 3);

        let var_one = call_native(
            &mut i,
            "List::Util::variance",
            &[PerlValue::integer(5)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(var_one.to_number(), 0.0);

        let var_pop = call_native(
            &mut i,
            "List::Util::variance",
            &[
                PerlValue::integer(2),
                PerlValue::integer(4),
                PerlValue::integer(6),
            ],
            WantarrayCtx::Scalar,
        );
        assert!((var_pop.to_number() - 8.0 / 3.0).abs() < 1e-9);

        let sd = call_native(
            &mut i,
            "List::Util::stddev",
            &[PerlValue::integer(0), PerlValue::integer(0)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(sd.to_number(), 0.0);
    }

    #[test]
    fn sum_product_min_max_list_context_returns_one_element_array() {
        let mut i = Interpreter::new();
        let args_sum = [
            PerlValue::integer(1),
            PerlValue::integer(2),
            PerlValue::integer(3),
        ];
        let ls = call_native(&mut i, "List::Util::sum", &args_sum, WantarrayCtx::List);
        let asum = ls.as_array_vec().expect("sum list");
        assert_eq!(asum.len(), 1);
        assert_eq!(asum[0].to_int(), 6);

        let lp = call_native(
            &mut i,
            "List::Util::product",
            &[PerlValue::integer(2), PerlValue::integer(4)],
            WantarrayCtx::List,
        );
        let ap = lp.as_array_vec().expect("product list");
        assert_eq!(ap.len(), 1);
        assert_eq!(ap[0].to_int(), 8);

        let lmn = call_native(
            &mut i,
            "List::Util::min",
            &[PerlValue::integer(9), PerlValue::integer(2)],
            WantarrayCtx::List,
        );
        assert_eq!(lmn.as_array_vec().unwrap()[0].to_int(), 2);
        let lmx = call_native(
            &mut i,
            "List::Util::max",
            &[PerlValue::integer(9), PerlValue::integer(2)],
            WantarrayCtx::List,
        );
        assert_eq!(lmx.as_array_vec().unwrap()[0].to_int(), 9);
    }

    #[test]
    fn min_max_empty_undef() {
        let mut i = Interpreter::new();
        let mn = call_native(&mut i, "List::Util::min", &[], WantarrayCtx::Scalar);
        assert!(mn.is_undef());
    }

    #[test]
    fn uniq_adjacent_strings() {
        let mut i = Interpreter::new();
        let u = call_native(
            &mut i,
            "List::Util::uniq",
            &[
                PerlValue::string("a".into()),
                PerlValue::string("a".into()),
                PerlValue::string("b".into()),
            ],
            WantarrayCtx::List,
        );
        let v = u.as_array_vec().expect("array");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].to_string(), "a");
        assert_eq!(v[1].to_string(), "b");
    }

    #[test]
    fn uniqstr_compares_strings_not_dwim() {
        let mut i = Interpreter::new();
        let u = call_native(
            &mut i,
            "List::Util::uniqstr",
            &[PerlValue::string("01".into()), PerlValue::integer(1)],
            WantarrayCtx::List,
        );
        let v = u.as_array_vec().expect("array");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn uniqint_coerces_to_int() {
        let mut i = Interpreter::new();
        let u = call_native(
            &mut i,
            "List::Util::uniqint",
            &[
                PerlValue::integer(2),
                PerlValue::integer(2),
                PerlValue::integer(3),
            ],
            WantarrayCtx::List,
        );
        let v = u.as_array_vec().expect("array");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].to_int(), 2);
        assert_eq!(v[1].to_int(), 3);
    }

    #[test]
    fn chunked_splits_list_last_arg_is_size() {
        let mut i = Interpreter::new();
        let c = call_native(
            &mut i,
            "List::Util::chunked",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(3),
                PerlValue::integer(4),
                PerlValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let rows = c.as_array_vec().expect("chunked list");
        assert_eq!(rows.len(), 2);
        let ar0 = rows[0].as_array_ref().expect("chunk");
        let r0 = ar0.read();
        assert_eq!(r0.len(), 2);
        assert_eq!(r0[0].to_int(), 1);
        assert_eq!(r0[1].to_int(), 2);
        let ar1 = rows[1].as_array_ref().expect("chunk");
        let r1 = ar1.read();
        assert_eq!(r1.len(), 2);
        assert_eq!(r1[0].to_int(), 3);
        assert_eq!(r1[1].to_int(), 4);

        let ns = call_native(
            &mut i,
            "List::Util::chunked",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(3),
                PerlValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(ns.to_int(), 2);
    }

    #[test]
    fn chunked_native_n_zero_and_empty_list() {
        let mut i = Interpreter::new();
        let z = call_native(
            &mut i,
            "List::Util::chunked",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(0),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(z.to_int(), 0);
        let zl = call_native(
            &mut i,
            "List::Util::chunked",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(0),
            ],
            WantarrayCtx::List,
        );
        assert!(zl.as_array_vec().is_some_and(|v| v.is_empty()));

        let only_n = call_native(
            &mut i,
            "List::Util::chunked",
            &[PerlValue::integer(5)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(only_n.to_int(), 0);
    }

    #[test]
    fn chunked_native_chunk_size_exceeds_length() {
        let mut i = Interpreter::new();
        let c = call_native(
            &mut i,
            "List::Util::chunked",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(99),
            ],
            WantarrayCtx::List,
        );
        let rows = c.as_array_vec().expect("chunks");
        assert_eq!(rows.len(), 1);
        let ar = rows[0].as_array_ref().expect("chunk");
        let r = ar.read();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].to_int(), 1);
        assert_eq!(r[1].to_int(), 2);
    }

    #[test]
    fn windowed_overlapping_windows() {
        let mut i = Interpreter::new();
        let w = call_native(
            &mut i,
            "List::Util::windowed",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(3),
                PerlValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let rows = w.as_array_vec().expect("windowed list");
        assert_eq!(rows.len(), 2);
        let ar0 = rows[0].as_array_ref().expect("win");
        let r0 = ar0.read();
        assert_eq!(r0.len(), 2);
        assert_eq!(r0[0].to_int(), 1);
        assert_eq!(r0[1].to_int(), 2);
        let ar1 = rows[1].as_array_ref().expect("win");
        let r1 = ar1.read();
        assert_eq!(r1[0].to_int(), 2);
        assert_eq!(r1[1].to_int(), 3);
    }

    #[test]
    fn windowed_zero_n_empty() {
        let mut i = Interpreter::new();
        let w = call_native(
            &mut i,
            "List::Util::windowed",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(0),
            ],
            WantarrayCtx::List,
        );
        assert!(w.as_array_vec().unwrap().is_empty());
    }

    #[test]
    fn windowed_n_larger_than_list_empty() {
        let mut i = Interpreter::new();
        let w = call_native(
            &mut i,
            "List::Util::windowed",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(5),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(w.to_int(), 0);
    }

    #[test]
    fn windowed_single_full_width_window() {
        let mut i = Interpreter::new();
        let w = call_native(
            &mut i,
            "List::Util::windowed",
            &[
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
                PerlValue::integer(3),
            ],
            WantarrayCtx::List,
        );
        let rows = w.as_array_vec().expect("one row");
        assert_eq!(rows.len(), 1);
        let ar = rows[0].as_array_ref().expect("win");
        let r = ar.read();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].to_int(), 10);
        assert_eq!(r[2].to_int(), 30);
    }

    #[test]
    fn head_and_tail() {
        let mut i = Interpreter::new();
        let h = call_native(
            &mut i,
            "List::Util::head",
            &[
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
                PerlValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let hv = h.as_array_vec().unwrap();
        assert_eq!(hv.len(), 2);
        assert_eq!(hv[0].to_int(), 10);
        let hs = call_native(
            &mut i,
            "List::Util::head",
            &[
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
                PerlValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(hs.to_int(), 20);
        let hn = call_native(
            &mut i,
            "List::Util::head",
            &[
                PerlValue::integer(1),
                PerlValue::integer(2),
                PerlValue::integer(3),
                PerlValue::integer(-1),
            ],
            WantarrayCtx::List,
        );
        let hnv = hn.as_array_vec().unwrap();
        assert_eq!(hnv.len(), 2);
        assert_eq!(hnv[0].to_int(), 1);
        assert_eq!(hnv[1].to_int(), 2);
        let t = call_native(
            &mut i,
            "List::Util::tail",
            &[
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
                PerlValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let tv = t.as_array_vec().unwrap();
        assert_eq!(tv.len(), 2);
        assert_eq!(tv[1].to_int(), 30);
        let ts = call_native(
            &mut i,
            "List::Util::tail",
            &[
                PerlValue::integer(10),
                PerlValue::integer(20),
                PerlValue::integer(30),
                PerlValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(ts.to_int(), 30);
    }

    #[test]
    fn pairkeys_and_pairvalues() {
        let mut i = Interpreter::new();
        let k = call_native(
            &mut i,
            "List::Util::pairkeys",
            &[
                PerlValue::string("a".into()),
                PerlValue::integer(1),
                PerlValue::string("b".into()),
                PerlValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let kv = k.as_array_vec().unwrap();
        assert_eq!(kv.len(), 2);
        assert_eq!(kv[0].to_string(), "a");
        assert_eq!(kv[1].to_string(), "b");
        let vals = call_native(
            &mut i,
            "List::Util::pairvalues",
            &[
                PerlValue::string("a".into()),
                PerlValue::integer(1),
                PerlValue::string("b".into()),
                PerlValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let vv = vals.as_array_vec().unwrap();
        assert_eq!(vv[0].to_int(), 1);
        assert_eq!(vv[1].to_int(), 2);
    }

    #[test]
    fn zip_shortest_two_lists() {
        let mut i = Interpreter::new();
        let z = call_native(
            &mut i,
            "List::Util::zip_shortest",
            &[
                PerlValue::array(vec![PerlValue::integer(1), PerlValue::integer(2)]),
                PerlValue::array(vec![PerlValue::integer(10)]),
            ],
            WantarrayCtx::List,
        );
        let rows = z.as_array_vec().unwrap();
        assert_eq!(rows.len(), 1);
        let row = rows[0].as_array_ref().expect("row ref");
        let g = row.read();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].to_int(), 1);
        assert_eq!(g[1].to_int(), 10);
    }

    #[test]
    fn mesh_interleaves_rows() {
        let mut i = Interpreter::new();
        let m = call_native(
            &mut i,
            "List::Util::mesh_shortest",
            &[
                PerlValue::array(vec![PerlValue::integer(1), PerlValue::integer(2)]),
                PerlValue::array(vec![PerlValue::integer(10), PerlValue::integer(20)]),
            ],
            WantarrayCtx::List,
        );
        let v = m.as_array_vec().unwrap();
        assert_eq!(v.len(), 4);
        assert_eq!(v[0].to_int(), 1);
        assert_eq!(v[1].to_int(), 10);
        assert_eq!(v[2].to_int(), 2);
        assert_eq!(v[3].to_int(), 20);
    }

    #[test]
    fn sample_without_pool_returns_empty() {
        let mut i = Interpreter::new();
        let s = call_native(
            &mut i,
            "List::Util::sample",
            &[PerlValue::integer(3)],
            WantarrayCtx::List,
        );
        let v = s.as_array_vec().unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn sub_util_set_subname_returns_coderef_arg() {
        let mut i = Interpreter::new();
        let cr = PerlValue::integer(42);
        let out = call_native(
            &mut i,
            "Sub::Util::set_subname",
            &[PerlValue::string("main::foo".into()), cr.clone()],
            WantarrayCtx::Scalar,
        );
        assert_eq!(out.to_int(), 42);
        let out2 = call_native(
            &mut i,
            "Sub::Util::subname",
            &[PerlValue::string("main::bar".into()), cr],
            WantarrayCtx::Scalar,
        );
        assert_eq!(out2.to_int(), 42);
    }
}
