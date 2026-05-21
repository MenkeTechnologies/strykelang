//! Stryke list builtins — native Rust implementations of `sum`, `min`, `max`, `uniq`,
//! `reduce`, `zip`, the `pairs` family, and friends. Every fn here is a bare-name builtin
//! reachable via [`dispatch_by_name`]; there is no Perl module emulation layer.

use std::sync::Arc;

/// All bare-name list builtins that shouldn't be shadowed by user code.
pub const LIST_BUILTIN_NAMES: &[&str] = &[
    "uniq",
    "uniqstr",
    "uniqint",
    "uniqnum",
    "sum",
    "sum0",
    "product",
    "mean",
    "median",
    "mode",
    "variance",
    "stddev",
    "min",
    "max",
    "minstr",
    "maxstr",
    "shuffle",
    "chunked",
    "windowed",
    "sample",
    "head",
    "tail",
    "take",
    "drop",
    "reduce",
    "fold",
    "reductions",
    "any",
    "all",
    "none",
    "notall",
    "first",
    "pairs",
    "unpairs",
    "pairkeys",
    "pairvalues",
    "pairgrep",
    "pairmap",
    "pairfirst",
    "zip",
    "zip_longest",
    "zip_shortest",
    "mesh",
    "mesh_longest",
    "mesh_shortest",
    "blessed",
    "refaddr",
    "reftype",
    "looks_like_number",
    "find_index",
    "firstidx",
    "first_index",
    "weaken",
    "unweaken",
    "isweak",
    "set_subname",
    "subname",
    "unicode_to_native",
];

/// Returns true if `name` is a list builtin that shouldn't be shadowed.
pub fn is_list_builtin_name(name: &str) -> bool {
    LIST_BUILTIN_NAMES.contains(&name)
}

use parking_lot::RwLock;
use rand::seq::SliceRandom;
use rand::Rng;

use crate::value::{BlessedRef, HeapObject, StrykeValue};
use crate::vm_helper::{ExecResult, VMHelper, WantarrayCtx};

/// Dispatch a list builtin by bare name. Stryke exposes every list builtin as a
/// bare-name builtin; callers pass the unqualified name (e.g. `"sum"`, `"reduce"`).
pub(crate) fn dispatch_by_name(
    interp: &mut VMHelper,
    name: &str,
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> Option<ExecResult> {
    match name {
        "uniq" => Some(dispatch_ok(uniq_with_want(args, want))),
        "uniqstr" => Some(dispatch_ok(uniqstr_with_want(args, want))),
        "uniqint" => Some(dispatch_ok(uniqint_with_want(args, want))),
        "uniqnum" => Some(dispatch_ok(uniqnum_with_want(args, want))),
        "sum" => Some(dispatch_ok(sum(args).map(|v| aggregate_wantarray(v, want)))),
        "sum0" => Some(dispatch_ok(
            sum0(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "product" => Some(dispatch_ok(
            product(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "mean" => Some(dispatch_ok(
            mean(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "median" => Some(dispatch_ok(
            median(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "mode" => Some(dispatch_ok(mode_with_want(args, want))),
        "variance" => Some(dispatch_ok(
            variance(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "stddev" => Some(dispatch_ok(
            stddev(args).map(|v| aggregate_wantarray(v, want)),
        )),
        "min" => Some(dispatch_ok(
            minmax(args, MinMax::MinNum).map(|v| aggregate_wantarray(v, want)),
        )),
        "max" => Some(dispatch_ok(
            minmax(args, MinMax::MaxNum).map(|v| aggregate_wantarray(v, want)),
        )),
        "minstr" => Some(dispatch_ok(
            minmax(args, MinMax::MinStr).map(|v| aggregate_wantarray(v, want)),
        )),
        "maxstr" => Some(dispatch_ok(
            minmax(args, MinMax::MaxStr).map(|v| aggregate_wantarray(v, want)),
        )),
        "shuffle" => Some(dispatch_ok(shuffle_native(interp, args))),
        "chunked" => Some(dispatch_ok(chunked_with_want(args, want))),
        "windowed" => Some(dispatch_ok(windowed_with_want(args, want))),
        "sample" => Some(dispatch_ok(sample_native(interp, args))),
        "head" => Some(dispatch_ok(head_tail_take_impl(
            args,
            HeadTailTake::HeadByList,
            want,
        ))),
        "tail" => Some(dispatch_ok(head_tail_take_impl(
            args,
            HeadTailTake::TailByList,
            want,
        ))),
        "reduce" | "fold" => Some(reduce_like(interp, args, want, false)),
        "reductions" => Some(reduce_like(interp, args, want, true)),
        "any" => Some(any_all_none(interp, args, want, AnyMode::Any)),
        "all" => Some(any_all_none(interp, args, want, AnyMode::All)),
        "none" => Some(any_all_none(interp, args, want, AnyMode::None)),
        "notall" => Some(any_all_none(interp, args, want, AnyMode::NotAll)),
        "first" => Some(first_native(interp, args, want)),
        "find_index" | "firstidx" | "first_index" => Some(find_index_native(interp, args, want)),
        "pairs" => Some(dispatch_ok(pairs_native(args))),
        "unpairs" => Some(dispatch_ok(unpairs_native(args))),
        "pairkeys" => Some(dispatch_ok(pairkeys_values(true, args))),
        "pairvalues" => Some(dispatch_ok(pairkeys_values(false, args))),
        "pairgrep" => Some(pairgrep_map(interp, args, want, PairMode::Grep)),
        "pairmap" => Some(pairgrep_map(interp, args, want, PairMode::Map)),
        "pairfirst" => Some(pairgrep_map(interp, args, want, PairMode::First)),
        "zip" | "zip_longest" => Some(dispatch_ok(zip_mesh(args, ZipMesh::ZipLongest))),
        "zip_shortest" => Some(dispatch_ok(zip_mesh(args, ZipMesh::ZipShortest))),
        "mesh" | "mesh_longest" => Some(dispatch_ok(zip_mesh(args, ZipMesh::MeshLongest))),
        "mesh_shortest" => Some(dispatch_ok(zip_mesh(args, ZipMesh::MeshShortest))),
        // Pair-method dispatch (blessed `_Pair` arrayrefs from `pairs`).
        "_Pair::key" => Some(dispatch_ok(pair_accessor(args, 0))),
        "_Pair::value" => Some(dispatch_ok(pair_accessor(args, 1))),
        "_Pair::TO_JSON" => Some(dispatch_ok(pair_to_json(args))),
        // Introspection builtins (bare names, no module prefix).
        "blessed" => Some(dispatch_ok(scalar_util_blessed(args.first()))),
        "refaddr" => Some(dispatch_ok(scalar_util_refaddr(args.first()))),
        "reftype" => Some(dispatch_ok(scalar_util_reftype(args.first()))),
        "looks_like_number" => Some(dispatch_ok(scalar_util_looks_like_number(args.first()))),
        "weaken" | "unweaken" => Some(dispatch_ok(Ok(StrykeValue::UNDEF))),
        "isweak" => Some(dispatch_ok(Ok(StrykeValue::integer(0)))),
        // Subname helpers — return the coderef unchanged.
        "set_subname" | "subname" => Some(dispatch_ok(sub_util_set_subname(args))),
        // UTF-8 codepoint passthrough.
        "unicode_to_native" => Some(dispatch_ok(utf8_unicode_to_native(args.first()))),
        _ => None,
    }
}

/// `set_subname $name, $coderef` → returns `$coderef` (stryke does not rename closures).
fn sub_util_set_subname(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    Ok(args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
}

fn utf8_unicode_to_native(arg: Option<&StrykeValue>) -> crate::error::StrykeResult<StrykeValue> {
    let n = arg.map(|a| a.to_int()).unwrap_or(0);
    Ok(StrykeValue::integer(n))
}

fn scalar_util_blessed(arg: Option<&StrykeValue>) -> crate::error::StrykeResult<StrykeValue> {
    let Some(v) = arg else {
        return Ok(StrykeValue::UNDEF);
    };
    Ok(v.as_blessed_ref()
        .map(|b| StrykeValue::string(b.class.clone()))
        .unwrap_or(StrykeValue::UNDEF))
}

fn scalar_util_refaddr(arg: Option<&StrykeValue>) -> crate::error::StrykeResult<StrykeValue> {
    let Some(v) = arg else {
        return Ok(StrykeValue::UNDEF);
    };
    if v.is_undef() {
        return Ok(StrykeValue::UNDEF);
    }
    if v.with_heap(|_| ()).is_none() {
        return Ok(StrykeValue::UNDEF);
    }
    Ok(StrykeValue::integer(v.raw_bits() as i64))
}

fn scalar_util_reftype(arg: Option<&StrykeValue>) -> crate::error::StrykeResult<StrykeValue> {
    let Some(v) = arg else {
        return Ok(StrykeValue::UNDEF);
    };
    if v.is_undef() {
        return Ok(StrykeValue::UNDEF);
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
        t.map(|s| StrykeValue::string(s.to_string()))
    })
    .flatten()
    .unwrap_or(StrykeValue::UNDEF))
}

/// `Scalar::Util::looks_like_number` parity. Returns truthy when the scalar
/// stringifies into something Perl's `numify` would accept: optional sign,
/// integer/decimal digits with optional decimal point, optional `e/E` exponent.
/// Inf/Inf/NaN literals (case-insensitive, optional sign) also match.
/// `undef` returns 0; numeric IV/NV scalars short-circuit to 1.
fn scalar_util_looks_like_number(
    arg: Option<&StrykeValue>,
) -> crate::error::StrykeResult<StrykeValue> {
    let Some(v) = arg else {
        return Ok(StrykeValue::integer(0));
    };
    if v.is_undef() {
        return Ok(StrykeValue::integer(0));
    }
    if v.is_integer_like() || v.is_float_like() {
        return Ok(StrykeValue::integer(1));
    }
    let s = v.to_string();
    let s = s.trim();
    if s.is_empty() {
        return Ok(StrykeValue::integer(0));
    }
    // Allow inf / infinity / nan with optional sign.
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let body_lower = body.to_ascii_lowercase();
    if matches!(body_lower.as_str(), "inf" | "infinity" | "nan") {
        return Ok(StrykeValue::integer(1));
    }
    let ok = s.parse::<f64>().is_ok();
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

fn dispatch_ok(r: crate::error::StrykeResult<StrykeValue>) -> ExecResult {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(e.into()),
    }
}

/// Perl list context for these subs is a return **list** of one scalar (possibly `undef`).
#[inline]
/// Scalar reducers (`sum`, `product`, `min`, `max`, …) collapse a list to one
/// number. Perl's `List::Util` returns a *scalar* regardless of caller context;
/// wrapping the result in a 1-element array in list context broke arithmetic
/// (`0 + sum(...)` would numify the array ref to 1, while string interpolation
/// happened to print the wrapped scalar). Always return the scalar.
fn aggregate_wantarray(v: StrykeValue, _want: WantarrayCtx) -> StrykeValue {
    v
}

enum MinMax {
    MinNum,
    MaxNum,
    MinStr,
    MaxStr,
}

fn minmax(args: &[StrykeValue], mode: MinMax) -> crate::error::StrykeResult<StrykeValue> {
    let flat = flatten_to_values(args);
    if flat.is_empty() {
        return Ok(StrykeValue::UNDEF);
    }
    let mut it = flat.into_iter();
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

fn uniq_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    let a = uniq_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(StrykeValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

/// Adjacent-unique like Perl 5 `uniq` (DWIM string/undef; refs compared by string form).
///
/// Fix for BUG-126/140: a single arrayref argument (`uniq([1,1,2,2])` or
/// `uniq(\@arr)`) used to be treated as one atom because only plain
/// `Array` values (`as_array_vec`) were recognised — the `ArrayRef`
/// branch fell through to the scalar `else` and pushed the ref itself.
/// Now both plain arrays and arrayrefs unfold into their elements.
fn uniq_list(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let push_val = |x: &StrykeValue,
                    out: &mut Vec<StrykeValue>,
                    seen: &mut std::collections::HashSet<String>| {
        let key = x.to_string();
        if seen.insert(key) {
            out.push(x.clone());
        }
    };
    for arg in args {
        if arg.is_iterator() {
            let iter = arg.clone().into_iterator();
            while let Some(x) = iter.next_item() {
                push_val(&x, &mut out, &mut seen);
            }
        } else if let Some(arr) = arg.as_array_vec() {
            for x in &arr {
                push_val(x, &mut out, &mut seen);
            }
        } else if let Some(arr_ref) = arg.as_array_ref() {
            // BUG-126/140 fix — deref arrayref args so `uniq([1,1,2,2])`
            // unfolds to (1, 1, 2, 2) → (1, 2) instead of returning the
            // ref as a single atom.
            for x in arr_ref.read().iter() {
                push_val(x, &mut out, &mut seen);
            }
        } else {
            push_val(arg, &mut out, &mut seen);
        }
    }
    Ok(StrykeValue::array(out))
}

fn uniqstr_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    let a = uniqstr_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(StrykeValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

fn uniqstr_list(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
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
    Ok(StrykeValue::array(out))
}

fn uniqint_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    let a = uniqint_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(StrykeValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

fn uniqint_list(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let mut out = Vec::new();
    let mut prev: Option<i64> = None;
    let mut have = false;
    for x in args {
        let n = x.to_int();
        if !have || prev != Some(n) {
            out.push(StrykeValue::integer(n));
            prev = Some(n);
            have = true;
        }
    }
    Ok(StrykeValue::array(out))
}

fn num_eq(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    a == b
}

fn uniqnum_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    let a = uniqnum_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let Some(x) = a.as_array_vec() {
            return Ok(StrykeValue::integer(x.len() as i64));
        }
    }
    Ok(a)
}

fn uniqnum_list(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
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
    Ok(StrykeValue::array(out))
}

fn sum(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Ok(StrykeValue::UNDEF);
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
        } else if let Some(arr_ref) = x.as_array_ref() {
            for item in arr_ref.read().iter() {
                s += item.to_number();
            }
        } else {
            s += x.to_number();
        }
    }
    Ok(StrykeValue::float(s))
}

fn sum0(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
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
        } else if let Some(arr_ref) = x.as_array_ref() {
            for item in arr_ref.read().iter() {
                s += item.to_number();
            }
        } else {
            s += x.to_number();
        }
    }
    Ok(StrykeValue::float(s))
}

fn product(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
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
        } else if let Some(arr_ref) = x.as_array_ref() {
            for item in arr_ref.read().iter() {
                p *= item.to_number();
            }
        } else {
            p *= x.to_number();
        }
    }
    Ok(StrykeValue::float(p))
}

/// Flatten args: dereference array refs, expand arrays/iterators into a flat list of values.
fn flatten_to_values(args: &[StrykeValue]) -> Vec<StrykeValue> {
    let mut out = Vec::new();
    for x in args {
        if x.is_iterator() {
            let iter = x.clone().into_iterator();
            while let Some(item) = iter.next_item() {
                out.push(item);
            }
        } else if let Some(arr) = x.as_array_vec() {
            out.extend(arr);
        } else if let Some(arr_ref) = x.as_array_ref() {
            out.extend(arr_ref.read().iter().cloned());
        } else {
            out.push(x.clone());
        }
    }
    out
}

/// Flatten args: dereference array refs, expand arrays/iterators into a flat list of numbers.
fn flatten_to_numbers(args: &[StrykeValue]) -> Vec<f64> {
    let mut out = Vec::new();
    for x in args {
        if x.is_iterator() {
            let iter = x.clone().into_iterator();
            while let Some(item) = iter.next_item() {
                out.push(item.to_number());
            }
        } else if let Some(arr) = x.as_array_vec() {
            for item in arr {
                out.push(item.to_number());
            }
        } else if let Some(arr_ref) = x.as_array_ref() {
            for item in arr_ref.read().iter() {
                out.push(item.to_number());
            }
        } else {
            out.push(x.to_number());
        }
    }
    out
}

/// Arithmetic mean; empty list → `undef`.
fn mean(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let nums = flatten_to_numbers(args);
    if nums.is_empty() {
        return Ok(StrykeValue::UNDEF);
    }
    let s: f64 = nums.iter().sum();
    Ok(StrykeValue::float(s / nums.len() as f64))
}

/// Median (linear interpolation for even length). Empty list → `undef`.
fn median(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let mut v = flatten_to_numbers(args);
    if v.is_empty() {
        return Ok(StrykeValue::UNDEF);
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    let mid = if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    };
    Ok(StrykeValue::float(mid))
}

/// Values with highest frequency (ties all returned in list context). Empty list → `undef` / empty list.
fn mode_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    let flat = flatten_to_values(args);
    if flat.is_empty() {
        return Ok(match want {
            WantarrayCtx::List => StrykeValue::array(vec![]),
            WantarrayCtx::Scalar | WantarrayCtx::Void => StrykeValue::UNDEF,
        });
    }
    let nums: Vec<f64> = flat.iter().map(|x| x.to_number()).collect();
    let mut idx: Vec<usize> = (0..flat.len()).collect();
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
    let modes: Vec<StrykeValue> = mode_starts.into_iter().map(|ix| flat[ix].clone()).collect();
    let first = modes.first().cloned().unwrap_or(StrykeValue::UNDEF);
    Ok(match want {
        WantarrayCtx::List => StrykeValue::array(modes),
        WantarrayCtx::Scalar => first,
        WantarrayCtx::Void => StrykeValue::UNDEF,
    })
}

/// Population variance (divide by N). Empty → `undef`; one element → `0`.
fn variance(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let nums = flatten_to_numbers(args);
    if nums.is_empty() {
        return Ok(StrykeValue::UNDEF);
    }
    let n = nums.len() as f64;
    let mean_v: f64 = nums.iter().sum::<f64>() / n;
    let var: f64 = nums.iter().map(|x| (x - mean_v).powi(2)).sum::<f64>() / n;
    Ok(StrykeValue::float(var))
}

fn stddev(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let var = variance(args)?;
    if var.is_undef() {
        return Ok(StrykeValue::UNDEF);
    }
    Ok(StrykeValue::float(var.to_number().sqrt()))
}

fn shuffle_native(
    interp: &mut VMHelper,
    args: &[StrykeValue],
) -> crate::error::StrykeResult<StrykeValue> {
    let mut v: Vec<StrykeValue> = args.to_vec();
    v.shuffle(&mut interp.rand_rng);
    Ok(StrykeValue::array(v))
}

/// `chunked LIST, N` — last argument is chunk size; preceding values are the list. Returns a list of
/// arrayrefs (same shape as `zip` rows). Scalar context: number of chunks.
fn chunked_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(crate::error::StrykeError::runtime(
            "chunked: expected LIST, N",
            0,
        ));
    }
    // Last arg is always the chunk size N; everything before it is the list.
    // `chunked(3)` → N=3, empty list.  `chunked(@list, 2)` → N=2, list items.
    let n = args[args.len() - 1].to_int().max(0) as usize;
    let items: Vec<StrykeValue> = args[..args.len().saturating_sub(1)].to_vec();
    if n == 0 {
        return Ok(match want {
            WantarrayCtx::Scalar => StrykeValue::integer(0),
            _ => StrykeValue::array(vec![]),
        });
    }
    let mut chunk_refs = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let end = (i + n).min(items.len());
        chunk_refs.push(StrykeValue::array_ref(Arc::new(RwLock::new(
            items[i..end].to_vec(),
        ))));
        i = end;
    }
    let n_chunks = chunk_refs.len() as i64;
    let out = StrykeValue::array(chunk_refs);
    Ok(match want {
        WantarrayCtx::Scalar => StrykeValue::integer(n_chunks),
        _ => out,
    })
}

/// `windowed LIST, N` — last argument is window size; preceding values are the list. Overlapping
/// sliding windows (step 1), each window an arrayref like [`chunked_with_want`]. No partial trailing
/// windows. Scalar context: window count.
fn windowed_with_want(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(crate::error::StrykeError::runtime(
            "windowed: expected LIST, N",
            0,
        ));
    }
    // windowed @l == windowed @l, 2 — single arg is the list, window size defaults to 2
    let (n, items) = if args.len() == 1 {
        (2usize, args[0].to_list())
    } else {
        let n = args[args.len() - 1].to_int().max(0) as usize;
        let items: Vec<StrykeValue> = args[..args.len().saturating_sub(1)].to_vec();
        (n, items)
    };
    if n == 0 || items.len() < n {
        return Ok(match want {
            WantarrayCtx::Scalar => StrykeValue::integer(0),
            _ => StrykeValue::array(vec![]),
        });
    }
    let mut windows = Vec::new();
    for i in 0..=(items.len() - n) {
        windows.push(StrykeValue::array_ref(Arc::new(RwLock::new(
            items[i..i + n].to_vec(),
        ))));
    }
    let nw = windows.len() as i64;
    let out = StrykeValue::array(windows);
    Ok(match want {
        WantarrayCtx::Scalar => StrykeValue::integer(nw),
        _ => out,
    })
}

fn sample_native(
    interp: &mut VMHelper,
    args: &[StrykeValue],
) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let n = args[0].to_int().max(0) as usize;
    let mut pool: Vec<StrykeValue> = args[1..].to_vec();
    let mut out = Vec::new();
    for _ in 0..n {
        if pool.is_empty() {
            break;
        }
        let j = interp.rand_rng.gen_range(0..pool.len());
        out.push(pool.swap_remove(j));
    }
    Ok(StrykeValue::array(out))
}

#[derive(Clone, Copy)]
pub(crate) enum HeadTailTake {
    /// Builtin `take` / bare `head` — negative count is treated as zero (`max(0)`).
    Take,
    /// `head` — negative count means “all but last |k|”.
    HeadByList,
    /// `tail` — same size rules as Perl `tail`.
    TailByList,
}

/// Shared by [`crate::builtins::builtin_take`], bare `head`, and `head` / `tail`.
/// **Argument order:** list operands first, **count last** — `take(@l, N)`, `head(10,20,30,2)`.
/// A single argument is treated as **N** with an empty list (`take(3)` → empty).
/// List context: array slice; scalar context: last element of that slice, or `undef` if empty.
pub(crate) fn head_tail_take_impl(
    args: &[StrykeValue],
    kind: HeadTailTake,
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::Scalar => StrykeValue::UNDEF,
            _ => StrykeValue::array(vec![]),
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
        HeadTailTake::HeadByList | HeadTailTake::TailByList => {
            let size = raw;
            if size >= 0 {
                size.min(n).max(0) as usize
            } else {
                let k = (-size).min(n);
                (n - k) as usize
            }
        }
    };
    let out: Vec<StrykeValue> = match kind {
        HeadTailTake::Take | HeadTailTake::HeadByList => list.into_iter().take(take_n).collect(),
        HeadTailTake::TailByList => {
            let len = list.len();
            let skip = len.saturating_sub(take_n);
            list.into_iter().skip(skip).collect()
        }
    };
    Ok(match want {
        WantarrayCtx::Scalar => out.last().cloned().unwrap_or(StrykeValue::UNDEF),
        _ => StrykeValue::array(out),
    })
}

/// Builtin `tail` — last `$n` items; negative `$n` clamps to zero (empty). Operands are
/// **list values then count**: `tail(@l, N)`. One argument is the list with count defaulting to 1.
/// When the list is a single string containing newlines, split into lines first (Rust [`str::lines`] rules).
pub(crate) fn extension_tail_impl(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::Scalar => StrykeValue::UNDEF,
            _ => StrykeValue::array(vec![]),
        });
    }
    // tail @l == tail @l, 1 — single arg is the list, count defaults to 1
    let raw = if args.len() == 1 {
        1
    } else {
        args[args.len() - 1].to_int()
    };
    let mut list: Vec<StrykeValue> = if args.len() == 1 {
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
                .map(|ln| StrykeValue::string(ln.to_string()))
                .collect();
        }
    }
    let n = list.len() as i64;
    let take_n = raw.max(0).min(n).max(0) as usize;
    let len = list.len();
    let skip = len.saturating_sub(take_n);
    let out: Vec<StrykeValue> = list.into_iter().skip(skip).collect();
    Ok(match want {
        WantarrayCtx::Scalar => out.last().cloned().unwrap_or(StrykeValue::UNDEF),
        _ => StrykeValue::array(out),
    })
}

/// Builtin `drop` — skip the first `$n` items; negative `$n` clamps to zero. Operands are
/// **list values then count**: `drop(@l, N)`. One argument is the list with count defaulting to 1.
/// Same multiline-string line split as [`extension_tail_impl`].
pub(crate) fn extension_drop_impl(
    args: &[StrykeValue],
    want: WantarrayCtx,
) -> crate::error::StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Ok(match want {
            WantarrayCtx::Scalar => StrykeValue::UNDEF,
            _ => StrykeValue::array(vec![]),
        });
    }
    // drop @l == drop @l, 1 — single arg is the list, count defaults to 1
    let raw = if args.len() == 1 {
        1
    } else {
        args[args.len() - 1].to_int()
    };
    let mut list: Vec<StrykeValue> = if args.len() == 1 {
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
                .map(|ln| StrykeValue::string(ln.to_string()))
                .collect();
        }
    }
    let n = list.len();
    let skip_n = raw.max(0).min(n as i64) as usize;
    let out: Vec<StrykeValue> = list.into_iter().skip(skip_n).collect();
    Ok(match want {
        WantarrayCtx::Scalar => out.last().cloned().unwrap_or(StrykeValue::UNDEF),
        _ => StrykeValue::array(out),
    })
}

fn reduce_like(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    want: WantarrayCtx,
    reductions: bool,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::StrykeError::runtime(
                "reduce: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<StrykeValue> = args[1..].to_vec();
    if items.is_empty() {
        if reductions {
            return Ok(StrykeValue::array(vec![]));
        }
        return Ok(StrykeValue::UNDEF);
    }
    if items.len() == 1 {
        if reductions {
            return Ok(StrykeValue::array(vec![items[0].clone()]));
        }
        return Ok(items[0].clone());
    }
    let mut acc = items[0].clone();
    let mut chain: Vec<StrykeValue> = if reductions {
        vec![acc.clone()]
    } else {
        vec![]
    };
    for b in items.iter().skip(1) {
        interp.scope.set_sort_pair(acc.clone(), b.clone());
        acc = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        if reductions {
            chain.push(acc.clone());
        }
    }
    if reductions {
        if want == WantarrayCtx::Scalar {
            return Ok(chain.last().cloned().unwrap_or(StrykeValue::UNDEF));
        }
        return Ok(StrykeValue::array(chain));
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
    interp: &mut VMHelper,
    args: &[StrykeValue],
    _want: WantarrayCtx,
    mode: AnyMode,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::StrykeError::runtime(
                "any/all/none/notall: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<StrykeValue> = args[1..].to_vec();
    let empty_ok = matches!(mode, AnyMode::All | AnyMode::None);
    if items.is_empty() {
        return Ok(StrykeValue::integer(if empty_ok { 1 } else { 0 }));
    }
    for it in items {
        // Pass `it` as positional arg so stryke lambdas `fn ($x) { ... }`
        // see it via @_; `set_topic` keeps `_`-style block readers working.
        interp.scope.set_topic(it.clone());
        let v = interp.call_sub(&code, vec![it], WantarrayCtx::Scalar, 0)?;
        let t = v.is_true();
        match mode {
            AnyMode::Any if t => return Ok(StrykeValue::integer(1)),
            AnyMode::All if !t => return Ok(StrykeValue::integer(0)),
            AnyMode::None if t => return Ok(StrykeValue::integer(0)),
            AnyMode::NotAll if !t => return Ok(StrykeValue::integer(1)),
            _ => {}
        }
    }
    Ok(StrykeValue::integer(match mode {
        AnyMode::Any => 0,
        AnyMode::All => 1,
        AnyMode::None => 1,
        AnyMode::NotAll => 0,
    }))
}

fn first_native(interp: &mut VMHelper, args: &[StrykeValue], _want: WantarrayCtx) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::StrykeError::runtime(
                "first: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<StrykeValue> = args[1..].to_vec();
    for it in items {
        interp.scope.set_topic(it.clone());
        let v = interp.call_sub(&code, vec![it.clone()], WantarrayCtx::Scalar, 0)?;
        if v.is_true() {
            return Ok(it);
        }
    }
    Ok(StrykeValue::UNDEF)
}

/// `find_index { BLOCK } LIST` — index of the first item for which `BLOCK`
/// returns truthy, or `-1` if no item matches. Alias of List::MoreUtils
/// `firstidx`/`first_index`.
fn find_index_native(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    _want: WantarrayCtx,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::StrykeError::runtime(
                "find_index: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let items: Vec<StrykeValue> = args[1..].to_vec();
    for (i, it) in items.into_iter().enumerate() {
        interp.scope.set_topic(it.clone());
        let v = interp.call_sub(&code, vec![it.clone()], WantarrayCtx::Scalar, 0)?;
        if v.is_true() {
            return Ok(StrykeValue::integer(i as i64));
        }
    }
    Ok(StrykeValue::integer(-1))
}

fn pairs_native(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        let row = vec![args[i].clone(), args[i + 1].clone()];
        let ar = StrykeValue::array_ref(Arc::new(RwLock::new(row)));
        let b = StrykeValue::blessed(Arc::new(BlessedRef::new_blessed("Pair".to_string(), ar)));
        out.push(b);
        i += 2;
    }
    Ok(StrykeValue::array(out))
}

fn unpairs_native(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let mut out = Vec::new();
    for x in args {
        if let Some(r) = x.as_array_ref() {
            let g = r.read();
            out.push(g.first().cloned().unwrap_or(StrykeValue::UNDEF));
            out.push(g.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
        } else if let Some(b) = x.as_blessed_ref() {
            if b.class == "Pair" {
                let d = b.data.read();
                if let Some(r) = d.as_array_ref() {
                    let g = r.read();
                    out.push(g.first().cloned().unwrap_or(StrykeValue::UNDEF));
                    out.push(g.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
                }
            } else {
                out.push(StrykeValue::UNDEF);
                out.push(StrykeValue::UNDEF);
            }
        } else {
            out.push(StrykeValue::UNDEF);
            out.push(StrykeValue::UNDEF);
        }
    }
    Ok(StrykeValue::array(out))
}

fn pairkeys_values(keys: bool, args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
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
    Ok(StrykeValue::array(out))
}

enum PairMode {
    Grep,
    Map,
    First,
}

fn pairgrep_map(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    want: WantarrayCtx,
    mode: PairMode,
) -> ExecResult {
    let code = match args.first().and_then(|x| x.as_code_ref()) {
        Some(s) => s,
        _ => {
            return Err(crate::error::StrykeError::runtime(
                "pairgrep/pairmap/pairfirst: first argument must be a CODE reference",
                0,
            )
            .into());
        }
    };
    let flat: Vec<StrykeValue> = args[1..].to_vec();
    match mode {
        PairMode::Grep => {
            let mut out = Vec::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                let a = flat[i].clone();
                let b = flat[i + 1].clone();
                interp.scope.set_sort_pair(a.clone(), b.clone());
                let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
                if v.is_true() {
                    out.push(a);
                    out.push(b);
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(StrykeValue::integer((out.len() / 2) as i64));
            }
            Ok(StrykeValue::array(out))
        }
        PairMode::Map => {
            let mut out = Vec::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                interp
                    .scope
                    .set_sort_pair(flat[i].clone(), flat[i + 1].clone());
                let produced = interp.call_sub(&code, vec![], WantarrayCtx::List, 0)?;
                if let Some(items) = produced.as_array_vec() {
                    out.extend(items);
                } else {
                    out.push(produced);
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(StrykeValue::integer(out.len() as i64));
            }
            Ok(StrykeValue::array(out))
        }
        PairMode::First => {
            let mut i = 0;
            while i + 1 < flat.len() {
                let a = flat[i].clone();
                let b = flat[i + 1].clone();
                interp.scope.set_sort_pair(a.clone(), b.clone());
                let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
                if v.is_true() {
                    if want == WantarrayCtx::Scalar {
                        return Ok(StrykeValue::integer(1));
                    }
                    return Ok(StrykeValue::array(vec![a, b]));
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(StrykeValue::integer(0));
            }
            Ok(StrykeValue::array(vec![]))
        }
    }
}

fn pair_accessor(args: &[StrykeValue], idx: usize) -> crate::error::StrykeResult<StrykeValue> {
    let obj = args.first().ok_or_else(|| {
        crate::error::StrykeError::runtime("Pair::key/value: missing invocant", 0)
    })?;
    pair_field(obj, idx)
}

fn pair_field(obj: &StrykeValue, idx: usize) -> crate::error::StrykeResult<StrykeValue> {
    let b = obj
        .as_blessed_ref()
        .ok_or_else(|| crate::error::StrykeError::runtime("Pair::method: not a pair object", 0))?;
    if b.class != "Pair" {
        return Err(crate::error::StrykeError::runtime(
            "Pair::method: not a pair object",
            0,
        ));
    }
    let d = b.data.read();
    if let Some(r) = d.as_array_ref() {
        let g = r.read();
        return Ok(g.get(idx).cloned().unwrap_or(StrykeValue::UNDEF));
    }
    Err(crate::error::StrykeError::runtime(
        "Pair: internal data is not an ARRAY reference",
        0,
    ))
}

fn pair_to_json(args: &[StrykeValue]) -> crate::error::StrykeResult<StrykeValue> {
    let obj = args
        .first()
        .ok_or_else(|| crate::error::StrykeError::runtime("Pair::TO_JSON: missing invocant", 0))?;
    let k = pair_field(obj, 0)?;
    let v = pair_field(obj, 1)?;
    Ok(StrykeValue::array(vec![k, v]))
}

enum ZipMesh {
    ZipLongest,
    ZipShortest,
    MeshLongest,
    MeshShortest,
}

fn zip_mesh(args: &[StrykeValue], mode: ZipMesh) -> crate::error::StrykeResult<StrykeValue> {
    let arrays: Vec<Vec<StrykeValue>> = args.iter().map(arg_to_list).collect();
    if arrays.is_empty() {
        return Ok(StrykeValue::array(vec![]));
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
                    row.push(a.get(i).cloned().unwrap_or(StrykeValue::UNDEF));
                }
                out.push(StrykeValue::array_ref(Arc::new(RwLock::new(row))));
            }
            Ok(StrykeValue::array(out))
        }
        ZipMesh::MeshLongest | ZipMesh::MeshShortest => {
            let mut out = Vec::new();
            for i in 0..len {
                for a in &arrays {
                    out.push(a.get(i).cloned().unwrap_or(StrykeValue::UNDEF));
                }
            }
            Ok(StrykeValue::array(out))
        }
    }
}

fn arg_to_list(v: &StrykeValue) -> Vec<StrykeValue> {
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
    use crate::value::StrykeValue;
    use crate::vm_helper::{VMHelper, WantarrayCtx};

    fn call_native(
        interp: &mut VMHelper,
        name: &str,
        args: &[StrykeValue],
        want: WantarrayCtx,
    ) -> StrykeValue {
        match dispatch_by_name(interp, name, args, want) {
            Some(Ok(v)) => v,
            Some(Err(e)) => panic!("{:?}", e),
            None => panic!("not a stryke list builtin: {name}"),
        }
    }

    #[test]
    fn sum_and_product() {
        let mut i = VMHelper::new();
        let s = call_native(
            &mut i,
            "sum",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(s.to_int(), 6);
        let p = call_native(
            &mut i,
            "product",
            &[StrykeValue::integer(2), StrykeValue::integer(3)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(p.to_int(), 6);
    }

    #[test]
    fn sum_empty_is_undef_sum0_empty_is_zero() {
        let mut i = VMHelper::new();
        let s = call_native(&mut i, "sum", &[], WantarrayCtx::Scalar);
        assert!(s.is_undef());
        let z = call_native(&mut i, "sum0", &[], WantarrayCtx::Scalar);
        assert_eq!(z.to_int(), 0);
    }

    #[test]
    fn product_empty_is_one() {
        let mut i = VMHelper::new();
        let p = call_native(&mut i, "product", &[], WantarrayCtx::Scalar);
        assert_eq!(p.to_int(), 1);
    }

    #[test]
    fn min_max_minstr_maxstr() {
        let mut i = VMHelper::new();
        let mn = call_native(
            &mut i,
            "min",
            &[StrykeValue::float(3.0), StrykeValue::float(1.0)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(mn.to_int(), 1);
        let mx = call_native(
            &mut i,
            "max",
            &[StrykeValue::integer(3), StrykeValue::integer(9)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(mx.to_int(), 9);
        let ms = call_native(
            &mut i,
            "minstr",
            &[
                StrykeValue::string("z".into()),
                StrykeValue::string("a".into()),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(ms.to_string(), "a");
    }

    #[test]
    fn mean_median_mode_variance_stddev() {
        let mut i = VMHelper::new();
        assert!(call_native(&mut i, "mean", &[], WantarrayCtx::Scalar).is_undef());
        let m = call_native(
            &mut i,
            "mean",
            &[
                StrykeValue::integer(2),
                StrykeValue::integer(4),
                StrykeValue::integer(10),
            ],
            WantarrayCtx::Scalar,
        );
        assert!((m.to_number() - 16.0 / 3.0).abs() < 1e-9);

        let med_odd = call_native(
            &mut i,
            "median",
            &[
                StrykeValue::integer(3),
                StrykeValue::integer(1),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(med_odd.to_int(), 2);

        let med_even = call_native(
            &mut i,
            "median",
            &[
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
                StrykeValue::integer(40),
            ],
            WantarrayCtx::Scalar,
        );
        assert!((med_even.to_number() - 25.0).abs() < 1e-9);

        let mode_sc = call_native(
            &mut i,
            "mode",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(mode_sc.to_int(), 2);

        let mode_li = call_native(
            &mut i,
            "mode",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
                StrykeValue::integer(3),
            ],
            WantarrayCtx::List,
        );
        let mv = mode_li.as_array_vec().expect("mode list");
        assert_eq!(mv.len(), 2);
        assert_eq!(mv[0].to_int(), 2);
        assert_eq!(mv[1].to_int(), 3);

        let var_one = call_native(
            &mut i,
            "variance",
            &[StrykeValue::integer(5)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(var_one.to_number(), 0.0);

        let var_pop = call_native(
            &mut i,
            "variance",
            &[
                StrykeValue::integer(2),
                StrykeValue::integer(4),
                StrykeValue::integer(6),
            ],
            WantarrayCtx::Scalar,
        );
        assert!((var_pop.to_number() - 8.0 / 3.0).abs() < 1e-9);

        let sd = call_native(
            &mut i,
            "stddev",
            &[StrykeValue::integer(0), StrykeValue::integer(0)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(sd.to_number(), 0.0);
    }

    #[test]
    fn sum_product_min_max_list_context_returns_scalar() {
        // Mirrors Perl's `List::Util` — these reducers always return a scalar,
        // independent of caller context. Wrapping in a 1-element array (the
        // previous behavior) broke arithmetic on the result; see
        // `aggregate_wantarray` for the rationale.
        let mut i = VMHelper::new();
        let args_sum = [
            StrykeValue::integer(1),
            StrykeValue::integer(2),
            StrykeValue::integer(3),
        ];
        let ls = call_native(&mut i, "sum", &args_sum, WantarrayCtx::List);
        assert!(ls.as_array_vec().is_none(), "sum should not wrap in array");
        assert_eq!(ls.to_int(), 6);

        let lp = call_native(
            &mut i,
            "product",
            &[StrykeValue::integer(2), StrykeValue::integer(4)],
            WantarrayCtx::List,
        );
        assert!(
            lp.as_array_vec().is_none(),
            "product should not wrap in array"
        );
        assert_eq!(lp.to_int(), 8);

        let lmn = call_native(
            &mut i,
            "min",
            &[StrykeValue::integer(9), StrykeValue::integer(2)],
            WantarrayCtx::List,
        );
        assert_eq!(lmn.to_int(), 2);
        let lmx = call_native(
            &mut i,
            "max",
            &[StrykeValue::integer(9), StrykeValue::integer(2)],
            WantarrayCtx::List,
        );
        assert_eq!(lmx.to_int(), 9);
    }

    #[test]
    fn min_max_empty_undef() {
        let mut i = VMHelper::new();
        let mn = call_native(&mut i, "min", &[], WantarrayCtx::Scalar);
        assert!(mn.is_undef());
    }

    #[test]
    fn uniq_adjacent_strings() {
        let mut i = VMHelper::new();
        let u = call_native(
            &mut i,
            "uniq",
            &[
                StrykeValue::string("a".into()),
                StrykeValue::string("a".into()),
                StrykeValue::string("b".into()),
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
        let mut i = VMHelper::new();
        let u = call_native(
            &mut i,
            "uniqstr",
            &[StrykeValue::string("01".into()), StrykeValue::integer(1)],
            WantarrayCtx::List,
        );
        let v = u.as_array_vec().expect("array");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn uniqint_coerces_to_int() {
        let mut i = VMHelper::new();
        let u = call_native(
            &mut i,
            "uniqint",
            &[
                StrykeValue::integer(2),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
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
        let mut i = VMHelper::new();
        let c = call_native(
            &mut i,
            "chunked",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
                StrykeValue::integer(4),
                StrykeValue::integer(2),
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
            "chunked",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(ns.to_int(), 2);
    }

    #[test]
    fn chunked_native_n_zero_and_empty_list() {
        let mut i = VMHelper::new();
        let z = call_native(
            &mut i,
            "chunked",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(0),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(z.to_int(), 0);
        let zl = call_native(
            &mut i,
            "chunked",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(0),
            ],
            WantarrayCtx::List,
        );
        assert!(zl.as_array_vec().is_some_and(|v| v.is_empty()));

        let only_n = call_native(
            &mut i,
            "chunked",
            &[StrykeValue::integer(5)],
            WantarrayCtx::Scalar,
        );
        assert_eq!(only_n.to_int(), 0);
    }

    #[test]
    fn chunked_native_chunk_size_exceeds_length() {
        let mut i = VMHelper::new();
        let c = call_native(
            &mut i,
            "chunked",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(99),
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
        let mut i = VMHelper::new();
        let w = call_native(
            &mut i,
            "windowed",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
                StrykeValue::integer(2),
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
        let mut i = VMHelper::new();
        let w = call_native(
            &mut i,
            "windowed",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(0),
            ],
            WantarrayCtx::List,
        );
        assert!(w.as_array_vec().unwrap().is_empty());
    }

    #[test]
    fn windowed_n_larger_than_list_empty() {
        let mut i = VMHelper::new();
        let w = call_native(
            &mut i,
            "windowed",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(5),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(w.to_int(), 0);
    }

    #[test]
    fn windowed_single_full_width_window() {
        let mut i = VMHelper::new();
        let w = call_native(
            &mut i,
            "windowed",
            &[
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
                StrykeValue::integer(3),
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
        let mut i = VMHelper::new();
        let h = call_native(
            &mut i,
            "head",
            &[
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let hv = h.as_array_vec().unwrap();
        assert_eq!(hv.len(), 2);
        assert_eq!(hv[0].to_int(), 10);
        let hs = call_native(
            &mut i,
            "head",
            &[
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(hs.to_int(), 20);
        let hn = call_native(
            &mut i,
            "head",
            &[
                StrykeValue::integer(1),
                StrykeValue::integer(2),
                StrykeValue::integer(3),
                StrykeValue::integer(-1),
            ],
            WantarrayCtx::List,
        );
        let hnv = hn.as_array_vec().unwrap();
        assert_eq!(hnv.len(), 2);
        assert_eq!(hnv[0].to_int(), 1);
        assert_eq!(hnv[1].to_int(), 2);
        let t = call_native(
            &mut i,
            "tail",
            &[
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let tv = t.as_array_vec().unwrap();
        assert_eq!(tv.len(), 2);
        assert_eq!(tv[1].to_int(), 30);
        let ts = call_native(
            &mut i,
            "tail",
            &[
                StrykeValue::integer(10),
                StrykeValue::integer(20),
                StrykeValue::integer(30),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::Scalar,
        );
        assert_eq!(ts.to_int(), 30);
    }

    #[test]
    fn pairkeys_and_pairvalues() {
        let mut i = VMHelper::new();
        let k = call_native(
            &mut i,
            "pairkeys",
            &[
                StrykeValue::string("a".into()),
                StrykeValue::integer(1),
                StrykeValue::string("b".into()),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let kv = k.as_array_vec().unwrap();
        assert_eq!(kv.len(), 2);
        assert_eq!(kv[0].to_string(), "a");
        assert_eq!(kv[1].to_string(), "b");
        let vals = call_native(
            &mut i,
            "pairvalues",
            &[
                StrykeValue::string("a".into()),
                StrykeValue::integer(1),
                StrykeValue::string("b".into()),
                StrykeValue::integer(2),
            ],
            WantarrayCtx::List,
        );
        let vv = vals.as_array_vec().unwrap();
        assert_eq!(vv[0].to_int(), 1);
        assert_eq!(vv[1].to_int(), 2);
    }

    #[test]
    fn zip_shortest_two_lists() {
        let mut i = VMHelper::new();
        let z = call_native(
            &mut i,
            "zip_shortest",
            &[
                StrykeValue::array(vec![StrykeValue::integer(1), StrykeValue::integer(2)]),
                StrykeValue::array(vec![StrykeValue::integer(10)]),
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
        let mut i = VMHelper::new();
        let m = call_native(
            &mut i,
            "mesh_shortest",
            &[
                StrykeValue::array(vec![StrykeValue::integer(1), StrykeValue::integer(2)]),
                StrykeValue::array(vec![StrykeValue::integer(10), StrykeValue::integer(20)]),
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
        let mut i = VMHelper::new();
        let s = call_native(
            &mut i,
            "sample",
            &[StrykeValue::integer(3)],
            WantarrayCtx::List,
        );
        let v = s.as_array_vec().unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn sub_util_set_subname_returns_coderef_arg() {
        let mut i = VMHelper::new();
        let cr = StrykeValue::integer(42);
        let out = call_native(
            &mut i,
            "set_subname",
            &[StrykeValue::string("main::foo".into()), cr.clone()],
            WantarrayCtx::Scalar,
        );
        assert_eq!(out.to_int(), 42);
        let out2 = call_native(
            &mut i,
            "subname",
            &[StrykeValue::string("main::bar".into()), cr],
            WantarrayCtx::Scalar,
        );
        assert_eq!(out2.to_int(), 42);
    }
}
