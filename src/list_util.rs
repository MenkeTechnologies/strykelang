//! Perl 5 `List::Util` — core Perl ships an XS `List/Util.pm`; perlrs registers native Rust
//! implementations here so every `EXPORT_OK` name is callable and matches common Perl 5 semantics.

use std::sync::Arc;

use parking_lot::RwLock;
use rand::seq::SliceRandom;
use rand::Rng;

use crate::ast::Block;
use crate::interpreter::{ExecResult, Interpreter, WantarrayCtx};
use crate::value::{BlessedRef, PerlSub, PerlValue};

/// Insert placeholder subs (empty body) and route calls through [`native_dispatch`].
pub fn install_list_util(interp: &mut Interpreter) {
    let empty: Block = vec![];
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
    "none",
    "notall",
    "product",
    "reduce",
    "reductions",
    "sum",
    "sum0",
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
        "List::Util::sum" => Some(dispatch_ok(sum(args))),
        "List::Util::sum0" => Some(dispatch_ok(sum0(args))),
        "List::Util::product" => Some(dispatch_ok(product(args))),
        "List::Util::min" => Some(dispatch_ok(minmax(args, MinMax::MinNum))),
        "List::Util::max" => Some(dispatch_ok(minmax(args, MinMax::MaxNum))),
        "List::Util::minstr" => Some(dispatch_ok(minmax(args, MinMax::MinStr))),
        "List::Util::maxstr" => Some(dispatch_ok(minmax(args, MinMax::MaxStr))),
        "List::Util::shuffle" => Some(dispatch_ok(shuffle_native(interp, args))),
        "List::Util::sample" => Some(dispatch_ok(sample_native(interp, args))),
        "List::Util::head" => Some(dispatch_ok(head_tail(args, HeadTail::Head))),
        "List::Util::tail" => Some(dispatch_ok(head_tail(args, HeadTail::Tail))),
        "List::Util::reduce" => Some(reduce_like(interp, args, want, false)),
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
        _ => None,
    }
}

fn dispatch_ok(r: crate::error::PerlResult<PerlValue>) -> ExecResult {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(e.into()),
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
        return Ok(PerlValue::Undef);
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
        if let PerlValue::Array(ref x) = a {
            return Ok(PerlValue::Integer(x.len() as i64));
        }
    }
    Ok(a)
}

/// Adjacent-unique like Perl 5 `uniq` (DWIM string/undef; refs compared by string form).
fn uniq_list(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut prev: Option<PerlValue> = None;
    let mut have = false;
    for x in args.iter().cloned() {
        if !have || !same_dwim(prev.as_ref().unwrap(), &x) {
            out.push(x.clone());
            prev = Some(x);
            have = true;
        }
    }
    Ok(PerlValue::Array(out))
}

fn same_dwim(a: &PerlValue, b: &PerlValue) -> bool {
    match (a, b) {
        (PerlValue::Undef, PerlValue::Undef) => true,
        (PerlValue::Undef, _) | (_, PerlValue::Undef) => false,
        _ => a.to_string() == b.to_string(),
    }
}

fn uniqstr_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    let a = uniqstr_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let PerlValue::Array(ref x) = a {
            return Ok(PerlValue::Integer(x.len() as i64));
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
    Ok(PerlValue::Array(out))
}

fn uniqint_with_want(
    args: &[PerlValue],
    want: WantarrayCtx,
) -> crate::error::PerlResult<PerlValue> {
    let a = uniqint_list(args)?;
    if want == WantarrayCtx::Scalar {
        if let PerlValue::Array(ref x) = a {
            return Ok(PerlValue::Integer(x.len() as i64));
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
            out.push(PerlValue::Integer(n));
            prev = Some(n);
            have = true;
        }
    }
    Ok(PerlValue::Array(out))
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
        if let PerlValue::Array(ref x) = a {
            return Ok(PerlValue::Integer(x.len() as i64));
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
    Ok(PerlValue::Array(out))
}

fn sum(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::Undef);
    }
    let mut s = 0.0;
    for x in args {
        s += x.to_number();
    }
    Ok(PerlValue::Float(s))
}

fn sum0(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut s = 0.0;
    for x in args {
        s += x.to_number();
    }
    Ok(PerlValue::Float(s))
}

fn product(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut p = 1.0;
    for x in args {
        p *= x.to_number();
    }
    Ok(PerlValue::Float(p))
}

fn shuffle_native(
    interp: &mut Interpreter,
    args: &[PerlValue],
) -> crate::error::PerlResult<PerlValue> {
    let mut v: Vec<PerlValue> = args.to_vec();
    v.shuffle(&mut interp.rand_rng);
    Ok(PerlValue::Array(v))
}

fn sample_native(
    interp: &mut Interpreter,
    args: &[PerlValue],
) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::Array(vec![]));
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
    Ok(PerlValue::Array(out))
}

enum HeadTail {
    Head,
    Tail,
}

fn head_tail(args: &[PerlValue], mode: HeadTail) -> crate::error::PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::Array(vec![]));
    }
    let size = args[0].to_int();
    let list: Vec<PerlValue> = args[1..].to_vec();
    let n = list.len() as i64;
    let take = match mode {
        HeadTail::Head => {
            if size >= 0 {
                size.min(n).max(0) as usize
            } else {
                let k = (-size).min(n);
                (n - k) as usize
            }
        }
        HeadTail::Tail => {
            if size >= 0 {
                size.min(n).max(0) as usize
            } else {
                let k = (-size).min(n);
                (n - k) as usize
            }
        }
    };
    let len = list.len();
    let out = match mode {
        HeadTail::Head => list.into_iter().take(take).collect(),
        HeadTail::Tail => {
            let skip = len.saturating_sub(take);
            list.into_iter().skip(skip).collect()
        }
    };
    Ok(PerlValue::Array(out))
}

fn reduce_like(
    interp: &mut Interpreter,
    args: &[PerlValue],
    want: WantarrayCtx,
    reductions: bool,
) -> ExecResult {
    let code = match args.first() {
        Some(PerlValue::CodeRef(s)) => s.clone(),
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
            return Ok(PerlValue::Array(vec![]));
        }
        return Ok(PerlValue::Undef);
    }
    if items.len() == 1 {
        if reductions {
            return Ok(PerlValue::Array(vec![items[0].clone()]));
        }
        return Ok(items[0].clone());
    }
    let mut acc = items[0].clone();
    let mut chain: Vec<PerlValue> = if reductions {
        vec![acc.clone()]
    } else {
        vec![]
    };
    for i in 1..items.len() {
        let _ = interp.scope.set_scalar("a", acc.clone());
        let _ = interp.scope.set_scalar("b", items[i].clone());
        acc = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        if reductions {
            chain.push(acc.clone());
        }
    }
    if reductions {
        if want == WantarrayCtx::Scalar {
            return Ok(chain.last().cloned().unwrap_or(PerlValue::Undef));
        }
        return Ok(PerlValue::Array(chain));
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
    let code = match args.first() {
        Some(PerlValue::CodeRef(s)) => s.clone(),
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
        return Ok(PerlValue::Integer(if empty_ok { 1 } else { 0 }));
    }
    for it in items {
        let _ = interp.scope.set_scalar("_", it);
        let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        let t = v.is_true();
        match mode {
            AnyMode::Any if t => return Ok(PerlValue::Integer(1)),
            AnyMode::All if !t => return Ok(PerlValue::Integer(0)),
            AnyMode::None if t => return Ok(PerlValue::Integer(0)),
            AnyMode::NotAll if !t => return Ok(PerlValue::Integer(1)),
            _ => {}
        }
    }
    Ok(PerlValue::Integer(match mode {
        AnyMode::Any => 0,
        AnyMode::All => 1,
        AnyMode::None => 1,
        AnyMode::NotAll => 0,
    }))
}

fn first_native(interp: &mut Interpreter, args: &[PerlValue], _want: WantarrayCtx) -> ExecResult {
    let code = match args.first() {
        Some(PerlValue::CodeRef(s)) => s.clone(),
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
        let _ = interp.scope.set_scalar("_", it.clone());
        let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
        if v.is_true() {
            return Ok(it);
        }
    }
    Ok(PerlValue::Undef)
}

fn pairs_native(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        let row = vec![args[i].clone(), args[i + 1].clone()];
        let ar = PerlValue::ArrayRef(Arc::new(RwLock::new(row)));
        let b = PerlValue::Blessed(Arc::new(BlessedRef {
            class: "List::Util::_Pair".to_string(),
            data: RwLock::new(ar),
        }));
        out.push(b);
        i += 2;
    }
    Ok(PerlValue::Array(out))
}

fn unpairs_native(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let mut out = Vec::new();
    for x in args.iter().cloned() {
        match x {
            PerlValue::ArrayRef(r) => {
                let g = r.read();
                out.push(g.first().cloned().unwrap_or(PerlValue::Undef));
                out.push(g.get(1).cloned().unwrap_or(PerlValue::Undef));
            }
            PerlValue::Blessed(b) if b.class == "List::Util::_Pair" => {
                let d = b.data.read();
                if let PerlValue::ArrayRef(r) = &*d {
                    let g = r.read();
                    out.push(g.first().cloned().unwrap_or(PerlValue::Undef));
                    out.push(g.get(1).cloned().unwrap_or(PerlValue::Undef));
                }
            }
            _ => {
                out.push(PerlValue::Undef);
                out.push(PerlValue::Undef);
            }
        }
    }
    Ok(PerlValue::Array(out))
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
    Ok(PerlValue::Array(out))
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
    let code = match args.first() {
        Some(PerlValue::CodeRef(s)) => s.clone(),
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
                let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
                if v.is_true() {
                    out.push(a);
                    out.push(b);
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(PerlValue::Integer((out.len() / 2) as i64));
            }
            Ok(PerlValue::Array(out))
        }
        PairMode::Map => {
            let mut out = Vec::new();
            let mut i = 0;
            while i + 1 < flat.len() {
                let _ = interp.scope.set_scalar("a", flat[i].clone());
                let _ = interp.scope.set_scalar("b", flat[i + 1].clone());
                let produced = interp.call_sub(&code, vec![], WantarrayCtx::List, 0)?;
                match produced {
                    PerlValue::Array(items) => out.extend(items),
                    other => out.push(other),
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(PerlValue::Integer(out.len() as i64));
            }
            Ok(PerlValue::Array(out))
        }
        PairMode::First => {
            let mut i = 0;
            while i + 1 < flat.len() {
                let a = flat[i].clone();
                let b = flat[i + 1].clone();
                let _ = interp.scope.set_scalar("a", a.clone());
                let _ = interp.scope.set_scalar("b", b.clone());
                let v = interp.call_sub(&code, vec![], WantarrayCtx::Scalar, 0)?;
                if v.is_true() {
                    if want == WantarrayCtx::Scalar {
                        return Ok(PerlValue::Integer(1));
                    }
                    return Ok(PerlValue::Array(vec![a, b]));
                }
                i += 2;
            }
            if want == WantarrayCtx::Scalar {
                return Ok(PerlValue::Integer(0));
            }
            Ok(PerlValue::Array(vec![]))
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
    match obj {
        PerlValue::Blessed(b) if b.class == "List::Util::_Pair" => {
            let d = b.data.read();
            if let PerlValue::ArrayRef(r) = &*d {
                let g = r.read();
                return Ok(g.get(idx).cloned().unwrap_or(PerlValue::Undef));
            }
            Err(crate::error::PerlError::runtime(
                "List::Util::_Pair: internal data is not an ARRAY reference",
                0,
            ))
        }
        _ => Err(crate::error::PerlError::runtime(
            "List::Util::_Pair::method: not a pair object",
            0,
        )),
    }
}

fn pair_to_json(args: &[PerlValue]) -> crate::error::PerlResult<PerlValue> {
    let obj = args.first().ok_or_else(|| {
        crate::error::PerlError::runtime("List::Util::_Pair::TO_JSON: missing invocant", 0)
    })?;
    let k = pair_field(obj, 0)?;
    let v = pair_field(obj, 1)?;
    Ok(PerlValue::Array(vec![k, v]))
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
        return Ok(PerlValue::Array(vec![]));
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
                    row.push(a.get(i).cloned().unwrap_or(PerlValue::Undef));
                }
                out.push(PerlValue::ArrayRef(Arc::new(RwLock::new(row))));
            }
            Ok(PerlValue::Array(out))
        }
        ZipMesh::MeshLongest | ZipMesh::MeshShortest => {
            let mut out = Vec::new();
            for i in 0..len {
                for a in &arrays {
                    out.push(a.get(i).cloned().unwrap_or(PerlValue::Undef));
                }
            }
            Ok(PerlValue::Array(out))
        }
    }
}

fn arg_to_list(v: &PerlValue) -> Vec<PerlValue> {
    match v {
        PerlValue::Array(a) => a.clone(),
        PerlValue::ArrayRef(r) => r.read().clone(),
        _ => vec![v.clone()],
    }
}
