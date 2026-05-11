//! `getopts` — Getopt::Long-style CLI flag parser.
//!
//! Usage:
//!
//! ```perl
//! my %opts = getopts(\@ARGV, [
//!     "verbose|v",         # bool flag
//!     "file|f=s",          # required string arg
//!     "count|n=i",          # required int arg
//!     "rate=f",            # required float arg
//!     "out:s",             # optional string arg
//!     "tag|t=s@",          # repeatable → arrayref
//!     "define|D=s%",       # --define key=val → hashref
//!     "debug+",            # incremental counter (-ddd → 3)
//!     "color!",            # negatable (--no-color)
//! ]);
//! ```
//!
//! Spec language (subset of Perl's Getopt::Long):
//!
//! - `name`                  bool flag, present = 1
//! - `name|alias|alias`      same option, multiple names; first is canonical
//! - `name=s` / `name=i` / `name=f`        required arg of string/int/float
//! - `name:s` / `name:i` / `name:f`        optional arg (defaults to "" / 0 / 0.0)
//! - `name=s@` etc.          repeatable: collects into an arrayref
//! - `name=s%` etc.          `--name key=val` collects into a hashref
//! - `name!`                 negatable bool; `--no-NAME` (or `--no-NAME`) → 0
//! - `name+`                 incremental: each occurrence increments by 1
//!
//! Parsing rules:
//!
//! - `--name`, `--name=value`, `--name value` for long options
//! - `-n`, `-n value`, `-nvalue` for short options (single-char names)
//! - `--` terminates option parsing
//! - Unknown option → runtime error
//! - Bad type (`--count=abc` for `=i`) → runtime error
//! - First positional that isn't an option terminates parsing (no intermixed)
//!
//! If the first argument is an array ref (`\@ARGV`), it is mutated in place
//! to contain only the leftover positional arguments.
//!
//! Output: a hash with the canonical name as key. Booleans default to 0,
//! counters default to 0, repeatable specs default to `[]`, `=s%` to `{}`.
//! Scalar specs with no occurrence are absent from the hash unless given a
//! default via the hash form:
//!
//! ```perl
//! my %opts = getopts(\@ARGV, {
//!     "verbose|v"  => 0,
//!     "count|n=i"  => 10,
//!     "tag|t=s@"   => [],
//! });
//! ```

use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScalarType {
    Str,
    Int,
    Float,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgKind {
    /// `name` — bool flag, no arg.
    Bool,
    /// `name!` — negatable bool, `--no-NAME` flips to 0.
    NegBool,
    /// `name+` — counter, each occurrence increments.
    Counter,
    /// `name=T` — required argument of the given scalar type.
    Required(ScalarType),
    /// `name:T` — optional argument; if absent uses zero-value of the type.
    Optional(ScalarType),
    /// `name=T@` — required, repeatable, collects into arrayref.
    Array(ScalarType),
    /// `name=T%` — `--name key=val`, collects into hashref.
    Hash(ScalarType),
}

#[derive(Clone, Debug)]
struct OptSpec {
    canonical: String,
    aliases: Vec<String>,
    kind: ArgKind,
    default: Option<PerlValue>,
}

impl OptSpec {
    /// All names recognised for this option, including the canonical name.
    fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.canonical.as_str()).chain(self.aliases.iter().map(|s| s.as_str()))
    }
}

fn parse_spec_string(spec: &str, line: usize) -> PerlResult<OptSpec> {
    let bytes = spec.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return Err(PerlError::runtime("getopts: empty spec string", line));
    }

    // Find where the type suffix begins. Type starts at the first `=`, `:`, `!`, or `+`.
    let mut split = len;
    for (i, b) in bytes.iter().enumerate() {
        if matches!(b, b'=' | b':' | b'!' | b'+') {
            split = i;
            break;
        }
    }
    let names_part = &spec[..split];
    let type_part = &spec[split..];

    let mut names: Vec<String> = names_part.split('|').map(|s| s.to_string()).collect();
    if names.is_empty() || names.iter().any(|n| n.is_empty()) {
        return Err(PerlError::runtime(
            format!("getopts: invalid spec '{}': empty option name", spec),
            line,
        ));
    }
    let canonical = names.remove(0);

    let kind = match type_part {
        "" => ArgKind::Bool,
        "!" => ArgKind::NegBool,
        "+" => ArgKind::Counter,
        _ => {
            // Either `=T` or `:T` followed by optional `@` / `%`.
            let mut chars = type_part.chars();
            let first = chars.next().unwrap();
            let required = match first {
                '=' => true,
                ':' => false,
                _ => {
                    return Err(PerlError::runtime(
                        format!("getopts: invalid spec '{}': bad type marker", spec),
                        line,
                    ));
                }
            };
            let ty_char = chars.next().ok_or_else(|| {
                PerlError::runtime(
                    format!("getopts: invalid spec '{}': missing type", spec),
                    line,
                )
            })?;
            let ty = match ty_char {
                's' => ScalarType::Str,
                'i' => ScalarType::Int,
                'f' => ScalarType::Float,
                other => {
                    return Err(PerlError::runtime(
                        format!(
                            "getopts: invalid spec '{}': unknown type '{}'",
                            spec, other
                        ),
                        line,
                    ));
                }
            };
            let suffix: String = chars.collect();
            match suffix.as_str() {
                "" => {
                    if required {
                        ArgKind::Required(ty)
                    } else {
                        ArgKind::Optional(ty)
                    }
                }
                "@" => {
                    if !required {
                        return Err(PerlError::runtime(
                            format!("getopts: invalid spec '{}': `:T@` not supported (use `=T@`)", spec),
                            line,
                        ));
                    }
                    ArgKind::Array(ty)
                }
                "%" => {
                    if !required {
                        return Err(PerlError::runtime(
                            format!("getopts: invalid spec '{}': `:T%` not supported (use `=T%`)", spec),
                            line,
                        ));
                    }
                    ArgKind::Hash(ty)
                }
                other => {
                    return Err(PerlError::runtime(
                        format!("getopts: invalid spec '{}': trailing '{}'", spec, other),
                        line,
                    ));
                }
            }
        }
    };

    Ok(OptSpec {
        canonical,
        aliases: names,
        kind,
        default: None,
    })
}

fn coerce_scalar(raw: &str, ty: ScalarType, opt: &str, line: usize) -> PerlResult<PerlValue> {
    match ty {
        ScalarType::Str => Ok(PerlValue::string(raw.to_string())),
        ScalarType::Int => raw.parse::<i64>().map(PerlValue::integer).map_err(|_| {
            PerlError::runtime(
                format!("getopts: option '{}' expects integer, got '{}'", opt, raw),
                line,
            )
        }),
        ScalarType::Float => raw.parse::<f64>().map(PerlValue::float).map_err(|_| {
            PerlError::runtime(
                format!("getopts: option '{}' expects float, got '{}'", opt, raw),
                line,
            )
        }),
    }
}

fn zero_scalar(ty: ScalarType) -> PerlValue {
    match ty {
        ScalarType::Str => PerlValue::string(String::new()),
        ScalarType::Int => PerlValue::integer(0),
        ScalarType::Float => PerlValue::float(0.0),
    }
}

/// Render the option as it would appear on the command line.
fn render_opt(name: &str) -> String {
    if name.chars().count() == 1 {
        format!("-{}", name)
    } else {
        format!("--{}", name)
    }
}

/// Look up a spec by any of its names. Returns (index, matched-name, is-negation).
fn find_spec<'a>(
    specs: &'a [OptSpec],
    name: &str,
) -> Option<(usize, &'a OptSpec, bool)> {
    for (i, s) in specs.iter().enumerate() {
        for n in s.names() {
            if n == name {
                return Some((i, s, false));
            }
        }
        // `name!` accepts `no-NAME` as a negation
        if matches!(s.kind, ArgKind::NegBool) {
            for n in s.names() {
                if name == format!("no-{}", n) || name == format!("no{}", n) {
                    return Some((i, s, true));
                }
            }
        }
    }
    None
}

fn split_hash_kv(raw: &str, opt: &str, line: usize) -> PerlResult<(String, String)> {
    let eq = raw.find('=').ok_or_else(|| {
        PerlError::runtime(
            format!(
                "getopts: option '{}' expects key=value, got '{}'",
                opt, raw
            ),
            line,
        )
    })?;
    Ok((raw[..eq].to_string(), raw[eq + 1..].to_string()))
}

fn store_value(
    out: &mut IndexMap<String, PerlValue>,
    spec: &OptSpec,
    value: PerlValue,
    opt: &str,
    line: usize,
) -> PerlResult<()> {
    match spec.kind {
        ArgKind::Array(_) => {
            let entry = out
                .entry(spec.canonical.clone())
                .or_insert_with(|| PerlValue::array_ref(Arc::new(RwLock::new(Vec::new()))));
            let arc = entry.as_array_ref().ok_or_else(|| {
                PerlError::runtime(
                    format!("getopts: option '{}' internal: not an array ref", opt),
                    line,
                )
            })?;
            arc.write().push(value);
        }
        ArgKind::Hash(_) => {
            let entry = out
                .entry(spec.canonical.clone())
                .or_insert_with(|| PerlValue::hash_ref(Arc::new(RwLock::new(IndexMap::new()))));
            let arc = entry.as_hash_ref().ok_or_else(|| {
                PerlError::runtime(
                    format!("getopts: option '{}' internal: not a hash ref", opt),
                    line,
                )
            })?;
            // Hash entries are (key, val) where value is a 2-element array we
            // unpacked above; here we expect the caller to have given us a
            // 2-element PerlValue::array of [k, v].
            if let Some(pair) = value.as_array_vec() {
                if pair.len() == 2 {
                    arc.write().insert(pair[0].to_string(), pair[1].clone());
                    return Ok(());
                }
            }
            return Err(PerlError::runtime(
                format!("getopts: option '{}' internal: malformed hash kv", opt),
                line,
            ));
        }
        ArgKind::Counter => {
            let entry = out
                .entry(spec.canonical.clone())
                .or_insert_with(|| PerlValue::integer(0));
            let n = entry.to_int();
            *entry = PerlValue::integer(n + 1);
        }
        _ => {
            out.insert(spec.canonical.clone(), value);
        }
    }
    Ok(())
}

/// Core parser. Mutates `argv` in place: on return it holds only the
/// leftover positional arguments. Returns the hash of parsed options.
fn parse_argv(
    argv: &mut Vec<PerlValue>,
    specs: &[OptSpec],
    line: usize,
) -> PerlResult<IndexMap<String, PerlValue>> {
    let mut out: IndexMap<String, PerlValue> = IndexMap::new();

    // Seed defaults: counters → 0, arrays → [], hashes → {}, NegBool → undef (only set if seen).
    for s in specs {
        match s.kind {
            ArgKind::Counter => {
                out.insert(s.canonical.clone(), PerlValue::integer(0));
            }
            ArgKind::Array(_) => {
                out.insert(
                    s.canonical.clone(),
                    PerlValue::array_ref(Arc::new(RwLock::new(Vec::new()))),
                );
            }
            ArgKind::Hash(_) => {
                out.insert(
                    s.canonical.clone(),
                    PerlValue::hash_ref(Arc::new(RwLock::new(IndexMap::new()))),
                );
            }
            ArgKind::Bool | ArgKind::NegBool => {
                out.insert(s.canonical.clone(), PerlValue::integer(0));
            }
            _ => {}
        }
        if let Some(d) = &s.default {
            out.insert(s.canonical.clone(), d.clone());
        }
    }

    let input: Vec<String> = argv.iter().map(|v| v.to_string()).collect();
    let mut leftover: Vec<PerlValue> = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let arg = &input[i];

        // `--` terminator: everything else is positional.
        if arg == "--" {
            i += 1;
            while i < input.len() {
                leftover.push(PerlValue::string(input[i].clone()));
                i += 1;
            }
            break;
        }

        // Long option: --name or --name=value
        if let Some(rest) = arg.strip_prefix("--") {
            if rest.is_empty() {
                // `--` alone handled above; bare `--` shouldn't reach here, but treat as positional.
                leftover.push(PerlValue::string(arg.clone()));
                i += 1;
                continue;
            }
            let (name, inline_val) = match rest.find('=') {
                Some(eq) => (&rest[..eq], Some(rest[eq + 1..].to_string())),
                None => (rest, None),
            };
            let (_, spec, negated) = find_spec(specs, name).ok_or_else(|| {
                PerlError::runtime(
                    format!("getopts: unknown option --{}", name),
                    line,
                )
            })?;
            let display = render_opt(name);
            i = consume_option(
                &input,
                i,
                inline_val,
                spec,
                negated,
                &display,
                &mut out,
                line,
            )?;
            continue;
        }

        // Short option: -X, -X value, -Xvalue, -XYZ (bundled), -X=value
        if let Some(rest) = arg.strip_prefix('-') {
            if rest.is_empty() {
                // Bare `-` is a positional (conventional stdin marker).
                leftover.push(PerlValue::string(arg.clone()));
                i += 1;
                continue;
            }
            // Numeric (-5, -3.14) is a positional, not an option.
            if rest.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                leftover.push(PerlValue::string(arg.clone()));
                i += 1;
                continue;
            }

            // `-X=value` form: handle as a single short option with inline value.
            if let Some(eq) = rest.find('=') {
                let name = rest[..eq].to_string();
                let inline_val = Some(rest[eq + 1..].to_string());
                let (_, spec, negated) = find_spec(specs, &name).ok_or_else(|| {
                    PerlError::runtime(format!("getopts: unknown option -{}", name), line)
                })?;
                let display = render_opt(&name);
                i = consume_option(
                    &input,
                    i,
                    inline_val,
                    spec,
                    negated,
                    &display,
                    &mut out,
                    line,
                )?;
                continue;
            }

            // Bundled short options: walk char-by-char.
            // - Flag-style spec (Bool / NegBool / Counter): consume one char, continue.
            // - Arg-taking spec (Required / Optional / Array / Hash): consume the
            //   first char as the option name and the rest of the token (if any)
            //   as the value; otherwise pull the next argv token.
            let chars: Vec<char> = rest.chars().collect();
            let mut idx = 0usize;
            let mut advanced_outer = false;
            while idx < chars.len() {
                let key = chars[idx].to_string();
                let (_, spec, negated) = find_spec(specs, &key).ok_or_else(|| {
                    PerlError::runtime(format!("getopts: unknown option -{}", key), line)
                })?;
                let display = render_opt(&key);
                match spec.kind {
                    ArgKind::Bool | ArgKind::NegBool | ArgKind::Counter => {
                        // No-arg flag in the middle (or only one) of a bundle.
                        i = consume_option(
                            &input,
                            i,
                            None,
                            spec,
                            negated,
                            &display,
                            &mut out,
                            line,
                        )?;
                        // consume_option advanced `i` past the whole token; revert
                        // that advance for all but the last char in the bundle so
                        // the next char is parsed against the same input token.
                        if idx + 1 < chars.len() {
                            i -= 1;
                        } else {
                            advanced_outer = true;
                        }
                        idx += 1;
                    }
                    ArgKind::Required(_)
                    | ArgKind::Optional(_)
                    | ArgKind::Array(_)
                    | ArgKind::Hash(_) => {
                        let tail: String = chars[idx + 1..].iter().collect();
                        let inline_val = if tail.is_empty() { None } else { Some(tail) };
                        i = consume_option(
                            &input,
                            i,
                            inline_val,
                            spec,
                            negated,
                            &display,
                            &mut out,
                            line,
                        )?;
                        advanced_outer = true;
                        break;
                    }
                }
            }
            if !advanced_outer {
                // Defensive: should never reach here because at least one char in
                // the bundle was consumed and either advanced `i` or broke out.
                i += 1;
            }
            continue;
        }

        // Plain positional → stop parsing (no intermixed mode in v1).
        leftover.push(PerlValue::string(arg.clone()));
        i += 1;
        while i < input.len() {
            leftover.push(PerlValue::string(input[i].clone()));
            i += 1;
        }
    }

    *argv = leftover;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn consume_option(
    input: &[String],
    mut i: usize,
    inline_val: Option<String>,
    spec: &OptSpec,
    negated: bool,
    display: &str,
    out: &mut IndexMap<String, PerlValue>,
    line: usize,
) -> PerlResult<usize> {
    // i currently points at the option token; advance past it.
    i += 1;
    match spec.kind {
        ArgKind::Bool => {
            if inline_val.is_some() {
                return Err(PerlError::runtime(
                    format!("getopts: option {} takes no argument", display),
                    line,
                ));
            }
            out.insert(spec.canonical.clone(), PerlValue::integer(1));
        }
        ArgKind::NegBool => {
            if inline_val.is_some() {
                return Err(PerlError::runtime(
                    format!("getopts: option {} takes no argument", display),
                    line,
                ));
            }
            out.insert(
                spec.canonical.clone(),
                PerlValue::integer(if negated { 0 } else { 1 }),
            );
        }
        ArgKind::Counter => {
            if inline_val.is_some() {
                return Err(PerlError::runtime(
                    format!("getopts: option {} takes no argument", display),
                    line,
                ));
            }
            store_value(out, spec, PerlValue::UNDEF, display, line)?;
        }
        ArgKind::Required(ty) | ArgKind::Array(ty) => {
            let raw = match inline_val {
                Some(v) => v,
                None => {
                    if i >= input.len() {
                        return Err(PerlError::runtime(
                            format!("getopts: option {} requires a value", display),
                            line,
                        ));
                    }
                    let v = input[i].clone();
                    i += 1;
                    v
                }
            };
            let coerced = coerce_scalar(&raw, ty, display, line)?;
            store_value(out, spec, coerced, display, line)?;
        }
        ArgKind::Optional(ty) => {
            // Optional arg: take the next token only if it's inline or
            // doesn't start with `-`.
            let raw = match inline_val {
                Some(v) => Some(v),
                None => {
                    if i < input.len() && !input[i].starts_with('-') {
                        let v = input[i].clone();
                        i += 1;
                        Some(v)
                    } else {
                        None
                    }
                }
            };
            let coerced = match raw {
                Some(s) => coerce_scalar(&s, ty, display, line)?,
                None => zero_scalar(ty),
            };
            store_value(out, spec, coerced, display, line)?;
        }
        ArgKind::Hash(ty) => {
            let raw = match inline_val {
                Some(v) => v,
                None => {
                    if i >= input.len() {
                        return Err(PerlError::runtime(
                            format!("getopts: option {} requires key=value", display),
                            line,
                        ));
                    }
                    let v = input[i].clone();
                    i += 1;
                    v
                }
            };
            let (k, v) = split_hash_kv(&raw, display, line)?;
            let coerced = coerce_scalar(&v, ty, display, line)?;
            // Encode (k, v) as a 2-element array so store_value can unpack.
            let pair = PerlValue::array(vec![PerlValue::string(k), coerced]);
            // Reuse hash storage path.
            let _ = ty; // type already validated by coerce_scalar
            store_value(out, spec, pair, display, line)?;
        }
    }
    Ok(i)
}

/// Entry point. Signature:
///
/// ```text
/// getopts(ARGV_REF, SPECS)
/// ```
///
/// where `ARGV_REF` is `\@ARGV` (an array ref, mutated in place), and
/// `SPECS` is either:
///   - an arrayref of spec strings (no defaults), or
///   - a hashref mapping `spec => default-value`.
pub fn builtin_getopts(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "getopts: usage: getopts(\\@ARGV, [spec, ...]) or getopts(\\@ARGV, { spec => default })",
            line,
        ));
    }

    let argv_ref = args[0].as_array_ref().ok_or_else(|| {
        PerlError::runtime(
            "getopts: first argument must be an array reference (\\@ARGV)",
            line,
        )
    })?;

    // Collect specs from either arrayref or hashref form.
    let mut specs: Vec<OptSpec> = Vec::new();
    if let Some(arr) = args[1].as_array_ref() {
        for item in arr.read().iter() {
            let s = item.to_string();
            specs.push(parse_spec_string(&s, line)?);
        }
    } else if let Some(h) = args[1].as_hash_ref() {
        for (k, v) in h.read().iter() {
            let mut s = parse_spec_string(k, line)?;
            // Don't seed bool/counter/array/hash defaults from the hash form
            // unless the user explicitly provided one — but here every entry
            // in the hash IS explicit, so honor whatever was given.
            s.default = Some(v.clone());
            specs.push(s);
        }
    } else if let Some(arr) = args[1].as_array_vec() {
        // Plain array (not a ref) — accept too, for ergonomic call sites.
        for item in arr.iter() {
            let s = item.to_string();
            specs.push(parse_spec_string(&s, line)?);
        }
    } else {
        return Err(PerlError::runtime(
            "getopts: second argument must be an array ref of spec strings or a hash ref of spec => default",
            line,
        ));
    }

    // Detect duplicate canonical names — surfacing this is more useful than
    // silently last-write-wins.
    {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for s in &specs {
            if !seen.insert(&s.canonical) {
                return Err(PerlError::runtime(
                    format!(
                        "getopts: duplicate option name '{}' in specs",
                        s.canonical
                    ),
                    line,
                ));
            }
        }
    }

    // Take a writable copy of the argv contents, parse, then write back.
    let mut argv_copy: Vec<PerlValue> = argv_ref.read().clone();
    let result = parse_argv(&mut argv_copy, &specs, line)?;
    *argv_ref.write() = argv_copy;

    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(result))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> PerlValue {
        let v: Vec<PerlValue> = items.iter().map(|s| PerlValue::string((*s).into())).collect();
        PerlValue::array_ref(Arc::new(RwLock::new(v)))
    }

    fn specs(items: &[&str]) -> PerlValue {
        let v: Vec<PerlValue> = items.iter().map(|s| PerlValue::string((*s).into())).collect();
        PerlValue::array_ref(Arc::new(RwLock::new(v)))
    }

    fn hget(v: &PerlValue, k: &str) -> PerlValue {
        v.as_hash_ref()
            .expect("hash ref result")
            .read()
            .get(k)
            .cloned()
            .unwrap_or(PerlValue::UNDEF)
    }

    fn leftover(argv: &PerlValue) -> Vec<String> {
        argv.as_array_ref()
            .expect("argv ref")
            .read()
            .iter()
            .map(|v| v.to_string())
            .collect()
    }

    #[test]
    fn bool_flag_long_and_short() {
        let a = argv(&["--verbose", "rest"]);
        let s = specs(&["verbose|v"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(leftover(&a), vec!["rest".to_string()]);

        let a = argv(&["-v", "rest"]);
        let s = specs(&["verbose|v"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(leftover(&a), vec!["rest".to_string()]);
    }

    #[test]
    fn bool_default_zero_when_absent() {
        let a = argv(&["foo"]);
        let s = specs(&["verbose|v"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 0);
    }

    #[test]
    fn required_string_arg_separated_and_inline() {
        let a = argv(&["--file", "x.txt", "pos"]);
        let s = specs(&["file|f=s"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
        assert_eq!(leftover(&a), vec!["pos".to_string()]);

        let a = argv(&["--file=y.txt"]);
        let s = specs(&["file|f=s"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "y.txt");

        let a = argv(&["-fz.txt"]);
        let s = specs(&["file|f=s"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "z.txt");
    }

    #[test]
    fn int_and_float_coercion() {
        let a = argv(&["--count", "42", "--rate=3.14"]);
        let s = specs(&["count|n=i", "rate=f"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "count").to_int(), 42);
        assert!((hget(&out, "rate").to_number() - 3.14).abs() < 1e-9);
    }

    #[test]
    fn int_bad_value_errors() {
        let a = argv(&["--count", "abc"]);
        let s = specs(&["count=i"]);
        let err = builtin_getopts(&[a, s], 1).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("expects integer"), "msg was: {}", msg);
    }

    #[test]
    fn array_repeatable() {
        let a = argv(&["-t", "a", "--tag=b", "-t", "c"]);
        let s = specs(&["tag|t=s@"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        let v = hget(&out, "tag");
        let arr = v.as_array_ref().expect("arrayref").read().clone();
        let strs: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
        assert_eq!(strs, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn hash_kv() {
        let a = argv(&["-D", "k1=v1", "--define=k2=v2"]);
        let s = specs(&["define|D=s%"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        let v = hget(&out, "define");
        let h = v.as_hash_ref().expect("hashref").read().clone();
        assert_eq!(h.get("k1").map(|v| v.to_string()).as_deref(), Some("v1"));
        assert_eq!(h.get("k2").map(|v| v.to_string()).as_deref(), Some("v2"));
    }

    #[test]
    fn counter_plus() {
        let a = argv(&["--debug", "--debug", "--debug"]);
        let s = specs(&["debug+"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "debug").to_int(), 3);
    }

    #[test]
    fn negatable_bool() {
        let a = argv(&["--no-color"]);
        let s = specs(&["color!"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "color").to_int(), 0);

        let a = argv(&["--color"]);
        let s = specs(&["color!"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "color").to_int(), 1);
    }

    #[test]
    fn double_dash_terminator() {
        let a = argv(&["--verbose", "--", "--not-an-option", "pos"]);
        let s = specs(&["verbose"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(
            leftover(&a),
            vec!["--not-an-option".to_string(), "pos".to_string()]
        );
    }

    #[test]
    fn unknown_option_errors() {
        let a = argv(&["--nope"]);
        let s = specs(&["verbose"]);
        let err = builtin_getopts(&[a, s], 1).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown option --nope"), "msg was: {}", msg);
    }

    #[test]
    fn missing_required_value_errors() {
        let a = argv(&["--file"]);
        let s = specs(&["file=s"]);
        let err = builtin_getopts(&[a, s], 1).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("requires a value"), "msg was: {}", msg);
    }

    #[test]
    fn optional_value_absent_uses_zero() {
        let a = argv(&["--out", "--other"]);
        let s = specs(&["out:s", "other"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "out").to_string(), "");
        assert_eq!(hget(&out, "other").to_int(), 1);
    }

    #[test]
    fn defaults_via_hash_form() {
        let h: IndexMap<String, PerlValue> = [
            ("count|n=i".to_string(), PerlValue::integer(10)),
            ("file|f=s".to_string(), PerlValue::string("out.txt".into())),
        ]
        .into_iter()
        .collect();
        let spec = PerlValue::hash_ref(Arc::new(RwLock::new(h)));
        let a = argv(&[]);
        let out = builtin_getopts(&[a, spec], 1).unwrap();
        assert_eq!(hget(&out, "count").to_int(), 10);
        assert_eq!(hget(&out, "file").to_string(), "out.txt");
    }

    #[test]
    fn positional_stops_parsing() {
        let a = argv(&["--verbose", "input.txt", "--not-an-option"]);
        let s = specs(&["verbose"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(
            leftover(&a),
            vec!["input.txt".to_string(), "--not-an-option".to_string()]
        );
    }

    #[test]
    fn negative_number_is_positional() {
        let a = argv(&["-5", "-3.14"]);
        let s = specs(&["x=i"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        let _ = out;
        assert_eq!(leftover(&a), vec!["-5".to_string(), "-3.14".to_string()]);
    }

    #[test]
    fn duplicate_spec_errors() {
        let a = argv(&[]);
        let s = specs(&["verbose|v", "verbose=s"]);
        let err = builtin_getopts(&[a, s], 1).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("duplicate option name 'verbose'"), "msg was: {}", msg);
    }

    #[test]
    fn bundled_short_bools() {
        let a = argv(&["-vDR", "pos"]);
        let s = specs(&["verbose|v", "debug|D", "recursive|R"]);
        let out = builtin_getopts(&[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "debug").to_int(), 1);
        assert_eq!(hget(&out, "recursive").to_int(), 1);
        assert_eq!(leftover(&a), vec!["pos".to_string()]);
    }

    #[test]
    fn bundled_short_with_trailing_arg_taking_inline() {
        let a = argv(&["-vfx.txt"]);
        let s = specs(&["verbose|v", "file|f=s"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
    }

    #[test]
    fn bundled_short_with_trailing_arg_taking_separated() {
        let a = argv(&["-vf", "x.txt"]);
        let s = specs(&["verbose|v", "file|f=s"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
    }

    #[test]
    fn bundled_counter_repeated_char() {
        let a = argv(&["-vvv"]);
        let s = specs(&["verbose|v+"]);
        let out = builtin_getopts(&[a, s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 3);
    }

    #[test]
    fn bundle_unknown_char_errors() {
        let a = argv(&["-vXv"]);
        let s = specs(&["verbose|v"]);
        let err = builtin_getopts(&[a, s], 1).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown option -X"), "msg was: {}", msg);
    }
}
