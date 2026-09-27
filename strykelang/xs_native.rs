//! Core Perl XS     MODULES.iter().copied().find(|m| m.inc_key() == relpath)odules stryke provides natively: `List::Util`, `Scalar::Util`
//! and `POSIX`.
//!
//! In perl these modules' functions are C (XS) with no Perl body, so loading
//! their `.pm` from the system `@INC` gives stryke nothing to call — the file
//! ends in `XSLoader::load`, which needs the compiled `.bundle`. Instead,
//! `use`/`require` of one of these modules is satisfied here without reading
//! the `.pm`, and each function below is implemented in Rust:
//!
//! * `List::Util` / `Scalar::Util` reuse the list builtins in
//!   [`crate::list_builtins`], which implement the XS semantics.
//! * `POSIX` math functions return floating-point values like the C library
//!   (`POSIX::floor(2.5)` is the NV `2`, `floor(1e20)` stays `1e+20`), and its
//!   limits are the platform's `<limits.h>` / `<float.h>` values.
//!
//! Only the functions listed here exist. Importing any other name from these
//! modules is an error at `use` time rather than a sub that fails later.

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;
use crate::vm_helper::{ExecResult, VMHelper, WantarrayCtx};

/// How a call to an XS function parses when its prototype is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shape {
    /// `(@)` / `($@)` — an ordinary list operator.
    List,
    /// `(&@)` — `NAME BLOCK LIST`: a leading block is the code-ref argument.
    Block,
    /// `()` — a constant: a bare `NAME` takes no arguments (`INT_MAX + 1`).
    Nullary,
}

/// One natively implemented XS function.
pub(crate) struct XsFn {
    pub name: &'static str,
    pub shape: Shape,
}

/// A module whose functions are all native.
pub(crate) struct XsModule {
    pub name: &'static str,
    /// Names `use Module;` imports (the module's `@EXPORT`, restricted to the
    /// functions implemented here).
    pub default_export: &'static [&'static str],
    pub functions: &'static [XsFn],
    /// The Perl (non-XS) part of the module's `.pm`, run once when it loads.
    pub perl_source: &'static str,
}

impl XsModule {
    /// The module's `%INC` key: `"List/Util.pm"`.
    pub(crate) fn inc_key(&self) -> String {
        format!("{}.pm", self.name.replace("::", "/"))
    }

    pub(crate) fn function(&self, name: &str) -> Option<&'static XsFn> {
        self.functions.iter().find(|f| f.name == name)
    }
}

const fn list(name: &'static str) -> XsFn {
    XsFn {
        name,
        shape: Shape::List,
    }
}

const fn block(name: &'static str) -> XsFn {
    XsFn {
        name,
        shape: Shape::Block,
    }
}

const fn nullary(name: &'static str) -> XsFn {
    XsFn {
        name,
        shape: Shape::Nullary,
    }
}

static LIST_UTIL: XsModule = XsModule {
    name: "List::Util",
    default_export: &[],
    // List/Util.pm defines the methods of the objects `pairs` returns in Perl.
    perl_source: "sub List::Util::_Pair::key { shift->[0] }\n\
                  sub List::Util::_Pair::value { shift->[1] }\n\
                  sub List::Util::_Pair::TO_JSON { [ @{+shift} ] }\n",
    functions: &[
        block("all"),
        block("any"),
        block("first"),
        block("none"),
        block("notall"),
        block("reduce"),
        block("reductions"),
        block("pairgrep"),
        block("pairmap"),
        block("pairfirst"),
        list("min"),
        list("max"),
        list("minstr"),
        list("maxstr"),
        list("product"),
        list("sum"),
        list("sum0"),
        list("sample"),
        list("shuffle"),
        list("uniq"),
        list("uniqint"),
        list("uniqnum"),
        list("uniqstr"),
        list("zip"),
        list("zip_longest"),
        list("zip_shortest"),
        list("mesh"),
        list("mesh_longest"),
        list("mesh_shortest"),
        list("head"),
        list("tail"),
        list("pairs"),
        list("unpairs"),
        list("pairkeys"),
        list("pairvalues"),
    ],
};

static SCALAR_UTIL: XsModule = XsModule {
    name: "Scalar::Util",
    default_export: &[],
    perl_source: "",
    functions: &[
        list("blessed"),
        list("refaddr"),
        list("reftype"),
        list("looks_like_number"),
    ],
};

static POSIX: XsModule = XsModule {
    name: "POSIX",
    perl_source: "",
    default_export: &[
        "floor",
        "ceil",
        "fmod",
        "pow",
        "fabs",
        "log10",
        "tan",
        "acos",
        "asin",
        "atan",
        "cosh",
        "sinh",
        "tanh",
        "INT_MAX",
        "INT_MIN",
        "UINT_MAX",
        "LONG_MAX",
        "LONG_MIN",
        "DBL_MAX",
        "DBL_MIN",
        "DBL_EPSILON",
        "FLT_MAX",
        "EXIT_SUCCESS",
        "EXIT_FAILURE",
    ],
    functions: &[
        list("floor"),
        list("ceil"),
        list("fmod"),
        list("pow"),
        list("fabs"),
        list("log10"),
        list("tan"),
        list("acos"),
        list("asin"),
        list("atan"),
        list("cosh"),
        list("sinh"),
        list("tanh"),
        list("cbrt"),
        list("log2"),
        list("log1p"),
        list("expm1"),
        list("round"),
        list("trunc"),
        list("lround"),
        list("copysign"),
        list("fmin"),
        list("fmax"),
        list("hypot"),
        list("isnan"),
        list("isinf"),
        list("isfinite"),
        nullary("INT_MAX"),
        nullary("INT_MIN"),
        nullary("UINT_MAX"),
        nullary("LONG_MAX"),
        nullary("LONG_MIN"),
        nullary("DBL_MAX"),
        nullary("DBL_MIN"),
        nullary("DBL_EPSILON"),
        nullary("FLT_MAX"),
        nullary("EXIT_SUCCESS"),
        nullary("EXIT_FAILURE"),
    ],
};

static MODULES: [&XsModule; 3] = [&LIST_UTIL, &SCALAR_UTIL, &POSIX];

/// The native module named `name` (`"List::Util"`), if stryke provides one.
pub(crate) fn module(name: &str) -> Option<&'static XsModule> {
    MODULES.iter().copied().find(|m| m.name == name)
}

/// The native module a `require` path names (`"List/Util.pm"`).
pub(crate) fn module_for_inc_key(relpath: &str) -> Option<&'static XsModule> {
    MODULES.iter().copied().find(|m| m.inc_key() == relpath)
}

/// `List::Util::first` → the module and function, if it is native.
pub(crate) fn function(qualified: &str) -> Option<(&'static XsModule, &'static XsFn)> {
    let (pkg, short) = qualified.rsplit_once("::")?;
    let m = module(pkg)?;
    Some((m, m.function(short)?))
}

/// The short names `use Module LIST` imports. `LIST` empty means the default
/// export list; `()` (an explicit empty list) is handled by the caller.
/// Errors name the first entry that is not a native function of the module.
pub(crate) fn import_names(
    m: &'static XsModule,
    requested: &[String],
) -> Result<Vec<&'static str>, String> {
    if requested.is_empty() {
        return Ok(m.default_export.to_vec());
    }
    requested
        .iter()
        .map(|raw| {
            let short = raw.strip_prefix('&').unwrap_or(raw);
            m.function(short).map(|f| f.name).ok_or_else(|| {
                format!(
                    "\"{short}\" is not provided by stryke's native {} module",
                    m.name
                )
            })
        })
        .collect()
}

/// Call the native XS function `qualified` (`"POSIX::floor"`). `None` when the
/// name is not one of them.
pub(crate) fn call(
    interp: &mut VMHelper,
    qualified: &str,
    args: &[StrykeValue],
    want: WantarrayCtx,
    line: usize,
) -> Option<ExecResult> {
    let (m, f) = function(qualified)?;
    Some(match (m.name, f.name) {
        ("POSIX", name) => posix(name, args, line).map_err(Into::into),
        ("List::Util", "head") => Ok(head_tail(args, want, true)),
        ("List::Util", "tail") => Ok(head_tail(args, want, false)),
        ("List::Util", "pairs") => Ok(list_result(pairs(args), want)),
        (_, name) => match ScalarRule::of(name) {
            // One value in any context (`first` that finds nothing is `(undef)`).
            ScalarRule::Single => {
                crate::list_builtins::dispatch_by_name(interp, name, args, WantarrayCtx::Scalar)?
                    .map(|v| {
                        // The predicates return perl's yes/no (`1` / `""`).
                        let v = if matches!(name, "any" | "all" | "none" | "notall") {
                            StrykeValue::perl_bool(v.is_true())
                        } else {
                            v
                        };
                        match want {
                            WantarrayCtx::List => StrykeValue::array(vec![v]),
                            _ => v,
                        }
                    })
            }
            // The function itself counts in scalar context.
            ScalarRule::Count => crate::list_builtins::dispatch_by_name(interp, name, args, want)?,
            ScalarRule::Found => {
                crate::list_builtins::dispatch_by_name(interp, name, args, WantarrayCtx::List)?.map(
                    |v| match want {
                        WantarrayCtx::List => v,
                        // yes, or an empty return stack: `undef`.
                        _ if v.to_list().is_empty() => StrykeValue::UNDEF,
                        _ => StrykeValue::integer(1),
                    },
                )
            }
            ScalarRule::Last => {
                crate::list_builtins::dispatch_by_name(interp, name, args, WantarrayCtx::List)?
                    .map(|v| list_result(v, want))
            }
        },
    })
}

/// What a List::Util / Scalar::Util function returns in scalar context
/// (ListUtil.xs).
enum ScalarRule {
    /// Always exactly one value.
    Single,
    /// The number of elements (`uniq`) or pairs (`pairgrep`, `pairmap`).
    Count,
    /// `pairfirst`: `1` when a pair matched, else `undef`.
    Found,
    /// Any other list: the XS return stack collapses to its last element.
    Last,
}

impl ScalarRule {
    fn of(name: &str) -> Self {
        match name {
            "sum" | "sum0" | "product" | "min" | "max" | "minstr" | "maxstr" | "first" | "any"
            | "all" | "none" | "notall" | "reduce" | "blessed" | "refaddr" | "reftype"
            | "looks_like_number" => Self::Single,
            "uniq" | "uniqint" | "uniqnum" | "uniqstr" | "pairgrep" | "pairmap" => Self::Count,
            "pairfirst" => Self::Found,
            _ => Self::Last,
        }
    }
}

/// A list-returning XS function's value in `want` context.
fn list_result(list: StrykeValue, want: WantarrayCtx) -> StrykeValue {
    match want {
        WantarrayCtx::List => list,
        _ => list.to_list().pop().unwrap_or(StrykeValue::UNDEF),
    }
}

/// `head SIZE, LIST` / `tail SIZE, LIST`: the first (last) SIZE elements, or
/// with a negative SIZE all but the last (first) -SIZE. In scalar context the
/// XS function's list collapses to its last element.
fn head_tail(args: &[StrykeValue], want: WantarrayCtx, head: bool) -> StrykeValue {
    let size = args.first().map(|v| v.to_int()).unwrap_or(0);
    let list = args.get(1..).unwrap_or_default();
    let len = list.len() as i64;
    let n = if size >= 0 {
        size.min(len)
    } else {
        (len + size).max(0)
    } as usize;
    let picked = if head {
        &list[..n]
    } else {
        &list[list.len() - n..]
    };
    list_result(StrykeValue::array(picked.to_vec()), want)
}

/// `pairs LIST`: one `List::Util::_Pair` (a blessed two-element array) per
/// key/value pair; an odd trailing key pairs with `undef`.
fn pairs(args: &[StrykeValue]) -> StrykeValue {
    use crate::value::BlessedRef;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let out = args
        .chunks(2)
        .map(|kv| {
            let row = vec![
                kv[0].clone(),
                kv.get(1).cloned().unwrap_or(StrykeValue::UNDEF),
            ];
            let data = StrykeValue::array_ref(Arc::new(RwLock::new(row)));
            StrykeValue::blessed(Arc::new(BlessedRef::new_blessed(
                "List::Util::_Pair".to_string(),
                data,
            )))
        })
        .collect();
    StrykeValue::array(out)
}

fn num_arg(args: &[StrykeValue], i: usize) -> f64 {
    args.get(i).map(|v| v.to_number()).unwrap_or(0.0)
}

fn posix(name: &str, args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let unary = |f: fn(f64) -> f64| -> StrykeResult<StrykeValue> {
        if args.len() != 1 {
            return Err(StrykeError::runtime(
                format!("Usage: POSIX::{name}(x)"),
                line,
            ));
        }
        Ok(StrykeValue::float(f(num_arg(args, 0))))
    };
    let binary = |f: fn(f64, f64) -> f64| -> StrykeResult<StrykeValue> {
        if args.len() != 2 {
            return Err(StrykeError::runtime(
                format!("Usage: POSIX::{name}(x, y)"),
                line,
            ));
        }
        Ok(StrykeValue::float(f(num_arg(args, 0), num_arg(args, 1))))
    };
    let predicate = |f: fn(f64) -> bool| -> StrykeResult<StrykeValue> {
        if args.len() != 1 {
            return Err(StrykeError::runtime(
                format!("Usage: POSIX::{name}(x)"),
                line,
            ));
        }
        Ok(StrykeValue::integer(i64::from(f(num_arg(args, 0)))))
    };
    match name {
        "floor" => unary(f64::floor),
        "ceil" => unary(f64::ceil),
        "fabs" => unary(f64::abs),
        "log10" => unary(f64::log10),
        "log2" => unary(f64::log2),
        "log1p" => unary(f64::ln_1p),
        "expm1" => unary(f64::exp_m1),
        "cbrt" => unary(f64::cbrt),
        "tan" => unary(f64::tan),
        "acos" => unary(f64::acos),
        "asin" => unary(f64::asin),
        "atan" => unary(f64::atan),
        "cosh" => unary(f64::cosh),
        "sinh" => unary(f64::sinh),
        "tanh" => unary(f64::tanh),
        // C `round`: halfway cases away from zero, which is `f64::round`.
        "round" => unary(f64::round),
        "trunc" => unary(f64::trunc),
        // C `fmod`: the remainder has the sign of the dividend, which is `%` on f64.
        "fmod" => binary(|x, y| x % y),
        "pow" => binary(f64::powf),
        "copysign" => binary(f64::copysign),
        "fmin" => binary(f64::min),
        "fmax" => binary(f64::max),
        "hypot" => binary(f64::hypot),
        "isnan" => predicate(f64::is_nan),
        "isinf" => predicate(f64::is_infinite),
        "isfinite" => predicate(f64::is_finite),
        "lround" => {
            if args.len() != 1 {
                return Err(StrykeError::runtime("Usage: POSIX::lround(x)", line));
            }
            Ok(StrykeValue::integer(num_arg(args, 0).round() as i64))
        }
        "INT_MAX" => Ok(StrykeValue::integer(i64::from(libc::c_int::MAX))),
        "INT_MIN" => Ok(StrykeValue::integer(i64::from(libc::c_int::MIN))),
        "UINT_MAX" => Ok(StrykeValue::integer(i64::from(libc::c_uint::MAX))),
        "LONG_MAX" => Ok(StrykeValue::integer(libc::c_long::MAX as i64)),
        "LONG_MIN" => Ok(StrykeValue::integer(libc::c_long::MIN as i64)),
        "DBL_MAX" => Ok(StrykeValue::float(f64::MAX)),
        "DBL_MIN" => Ok(StrykeValue::float(f64::MIN_POSITIVE)),
        "DBL_EPSILON" => Ok(StrykeValue::float(f64::EPSILON)),
        "FLT_MAX" => Ok(StrykeValue::float(f64::from(f32::MAX))),
        "EXIT_SUCCESS" => Ok(StrykeValue::integer(i64::from(libc::EXIT_SUCCESS))),
        "EXIT_FAILURE" => Ok(StrykeValue::integer(i64::from(libc::EXIT_FAILURE))),
        other => Err(StrykeError::runtime(
            format!("internal: POSIX::{other} has no native implementation"),
            line,
        )),
    }
}
