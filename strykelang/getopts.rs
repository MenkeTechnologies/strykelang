//! `getopts` — Getopt::Long-style CLI flag parser.
//!
//! Usage (operates on `@ARGV` by default):
//!
//! ```perl
//! my %opts = getopts([
//!     "verbose|v",         # bool flag
//!     "file|f=s",          # required string arg
//!     "count|n=i",         # required int arg
//!     "rate=f",            # required float arg
//!     "out:s",             # optional string arg
//!     "tag|t=s@",          # repeatable → arrayref
//!     "define|D=s%",       # --define key=val → hashref
//!     "debug+",            # incremental counter (-ddd → 3)
//!     "color!",            # negatable (--no-color)
//! ]);
//!
//! # Explicit array ref to parse a list other than @ARGV:
//! my %opts = getopts(\@args, [ "verbose|v", "file|f=s" ]);
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
//! - `name!`                 negatable bool; `--no-NAME` → 0
//! - `name+`                 incremental: each occurrence increments by 1
//!
//! Parsing rules:
//!
//! - `--name`, `--name=value`, `--name value` for long options
//! - `-n`, `-n value`, `-nvalue` for short options (single-char names)
//! - Bundling: `-vDR` = `-v -D -R`; the rest of the bundle becomes the value
//!   of the first arg-taking option that appears in it (`-vfx.txt` → `-v -f x.txt`)
//! - `--` terminates option parsing
//! - Unknown option → runtime error
//! - Bad type (`--count=abc` for `=i`) → runtime error
//! - First positional that isn't an option terminates parsing (no intermixed)
//!
//! Output: a hash with the canonical name as key. Booleans default to 0,
//! counters default to 0, repeatable specs default to `[]`, `=s%` to `{}`.
//! Scalar specs with no occurrence are absent from the hash unless given a
//! default via the hash form:
//!
//! ```perl
//! my %opts = getopts({
//!     "verbose|v"  => 0,
//!     "count|n=i"  => 10,
//!     "tag|t=s@"   => [],
//! });
//! ```
//!
//! Per-option metadata (D1 form) and auto-`--help`:
//!
//! ```perl
//! my %opts = getopts({
//!     "verbose|v" => { help => "enable verbose" },
//!     "file|f=s"  => { help => "output path", default => "out.txt" },
//!     "count|n=i" => { help => "iterations", required => 1 },
//! }, { prog => "myscript", desc => "do a thing" });
//! ```
//!
//! When any spec carries `help` text and the user hasn't claimed their own
//! `--help`/`-h`, `getopts` intercepts `--help`/`-h`, prints a formatted
//! usage block, and `exit(0)`s.

use crate::error::{PerlError, PerlResult};
use crate::value::StrykeValue;
use crate::vm_helper::VMHelper;
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
    default: Option<StrykeValue>,
    help: Option<String>,
    required: bool,
    metavar: Option<String>,
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
        help: None,
        required: false,
        metavar: None,
    })
}

fn coerce_scalar(raw: &str, ty: ScalarType, opt: &str, line: usize) -> PerlResult<StrykeValue> {
    match ty {
        ScalarType::Str => Ok(StrykeValue::string(raw.to_string())),
        ScalarType::Int => raw.parse::<i64>().map(StrykeValue::integer).map_err(|_| {
            PerlError::runtime(
                format!("getopts: option '{}' expects integer, got '{}'", opt, raw),
                line,
            )
        }),
        ScalarType::Float => raw.parse::<f64>().map(StrykeValue::float).map_err(|_| {
            PerlError::runtime(
                format!("getopts: option '{}' expects float, got '{}'", opt, raw),
                line,
            )
        }),
    }
}

fn zero_scalar(ty: ScalarType) -> StrykeValue {
    match ty {
        ScalarType::Str => StrykeValue::string(String::new()),
        ScalarType::Int => StrykeValue::integer(0),
        ScalarType::Float => StrykeValue::float(0.0),
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
    out: &mut IndexMap<String, StrykeValue>,
    spec: &OptSpec,
    value: StrykeValue,
    opt: &str,
    line: usize,
) -> PerlResult<()> {
    match spec.kind {
        ArgKind::Array(_) => {
            let entry = out
                .entry(spec.canonical.clone())
                .or_insert_with(|| StrykeValue::array_ref(Arc::new(RwLock::new(Vec::new()))));
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
                .or_insert_with(|| StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new()))));
            let arc = entry.as_hash_ref().ok_or_else(|| {
                PerlError::runtime(
                    format!("getopts: option '{}' internal: not a hash ref", opt),
                    line,
                )
            })?;
            // Hash entries are (key, val) where value is a 2-element array we
            // unpacked above; here we expect the caller to have given us a
            // 2-element StrykeValue::array of [k, v].
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
                .or_insert_with(|| StrykeValue::integer(0));
            let n = entry.to_int();
            *entry = StrykeValue::integer(n + 1);
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
    argv: &mut Vec<StrykeValue>,
    specs: &[OptSpec],
    line: usize,
) -> PerlResult<IndexMap<String, StrykeValue>> {
    let mut out: IndexMap<String, StrykeValue> = IndexMap::new();

    // Seed defaults: counters → 0, arrays → [], hashes → {}, NegBool → undef (only set if seen).
    for s in specs {
        match s.kind {
            ArgKind::Counter => {
                out.insert(s.canonical.clone(), StrykeValue::integer(0));
            }
            ArgKind::Array(_) => {
                out.insert(
                    s.canonical.clone(),
                    StrykeValue::array_ref(Arc::new(RwLock::new(Vec::new()))),
                );
            }
            ArgKind::Hash(_) => {
                out.insert(
                    s.canonical.clone(),
                    StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new()))),
                );
            }
            ArgKind::Bool | ArgKind::NegBool => {
                out.insert(s.canonical.clone(), StrykeValue::integer(0));
            }
            _ => {}
        }
        if let Some(d) = &s.default {
            out.insert(s.canonical.clone(), d.clone());
        }
    }

    let input: Vec<String> = argv.iter().map(|v| v.to_string()).collect();
    let mut leftover: Vec<StrykeValue> = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let arg = &input[i];

        // `--` terminator: everything else is positional.
        if arg == "--" {
            i += 1;
            while i < input.len() {
                leftover.push(StrykeValue::string(input[i].clone()));
                i += 1;
            }
            break;
        }

        // Long option: --name or --name=value
        if let Some(rest) = arg.strip_prefix("--") {
            if rest.is_empty() {
                // `--` alone handled above; bare `--` shouldn't reach here, but treat as positional.
                leftover.push(StrykeValue::string(arg.clone()));
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
                leftover.push(StrykeValue::string(arg.clone()));
                i += 1;
                continue;
            }
            // Numeric (-5, -3.14) is a positional, not an option.
            if rest.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                leftover.push(StrykeValue::string(arg.clone()));
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
        leftover.push(StrykeValue::string(arg.clone()));
        i += 1;
        while i < input.len() {
            leftover.push(StrykeValue::string(input[i].clone()));
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
    out: &mut IndexMap<String, StrykeValue>,
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
            out.insert(spec.canonical.clone(), StrykeValue::integer(1));
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
                StrykeValue::integer(if negated { 0 } else { 1 }),
            );
        }
        ArgKind::Counter => {
            if inline_val.is_some() {
                return Err(PerlError::runtime(
                    format!("getopts: option {} takes no argument", display),
                    line,
                ));
            }
            store_value(out, spec, StrykeValue::UNDEF, display, line)?;
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
            let pair = StrykeValue::array(vec![StrykeValue::string(k), coerced]);
            // Reuse hash storage path.
            let _ = ty; // type already validated by coerce_scalar
            store_value(out, spec, pair, display, line)?;
        }
    }
    Ok(i)
}

#[derive(Default, Clone, Debug)]
struct HelpMeta {
    prog: Option<String>,
    desc: Option<String>,
    epilog: Option<String>,
}

fn parse_help_meta(v: &StrykeValue, line: usize) -> PerlResult<HelpMeta> {
    let h = v.as_hash_ref().ok_or_else(|| {
        PerlError::runtime(
            "getopts: third argument must be a hash ref { prog => ..., desc => ..., epilog => ... }",
            line,
        )
    })?;
    let mut meta = HelpMeta::default();
    for (k, val) in h.read().iter() {
        match k.as_str() {
            "prog" => meta.prog = Some(val.to_string()),
            "desc" | "description" => meta.desc = Some(val.to_string()),
            "epilog" => meta.epilog = Some(val.to_string()),
            other => {
                return Err(PerlError::runtime(
                    format!("getopts: unknown meta key '{}' (expected prog/desc/epilog)", other),
                    line,
                ));
            }
        }
    }
    Ok(meta)
}

/// Apply D1 metadata (a hashref value in the spec hash) to an `OptSpec`.
fn apply_metadata(
    spec: &mut OptSpec,
    meta: &Arc<RwLock<IndexMap<String, StrykeValue>>>,
    line: usize,
) -> PerlResult<()> {
    for (k, v) in meta.read().iter() {
        match k.as_str() {
            "help" => spec.help = Some(v.to_string()),
            "default" => spec.default = Some(v.clone()),
            "required" => spec.required = v.to_int() != 0,
            "metavar" => spec.metavar = Some(v.to_string()),
            other => {
                return Err(PerlError::runtime(
                    format!(
                        "getopts: unknown metadata key '{}' for spec '{}' (expected help/default/required/metavar)",
                        other, spec.canonical
                    ),
                    line,
                ));
            }
        }
    }
    Ok(())
}

fn default_metavar(kind: ArgKind, custom: &Option<String>) -> String {
    if let Some(m) = custom {
        return m.clone();
    }
    match kind {
        ArgKind::Required(ScalarType::Str)
        | ArgKind::Optional(ScalarType::Str)
        | ArgKind::Array(ScalarType::Str) => "VALUE".to_string(),
        ArgKind::Required(ScalarType::Int)
        | ArgKind::Optional(ScalarType::Int)
        | ArgKind::Array(ScalarType::Int) => "N".to_string(),
        ArgKind::Required(ScalarType::Float)
        | ArgKind::Optional(ScalarType::Float)
        | ArgKind::Array(ScalarType::Float) => "X".to_string(),
        ArgKind::Hash(_) => "KEY=VAL".to_string(),
        _ => String::new(),
    }
}

fn format_left_col(s: &OptSpec) -> String {
    let mut shorts: Vec<&str> = Vec::new();
    let mut longs: Vec<&str> = Vec::new();
    for n in s.names() {
        if n.chars().count() == 1 {
            shorts.push(n);
        } else {
            longs.push(n);
        }
    }
    let mut parts: Vec<String> = Vec::new();
    for sh in &shorts {
        parts.push(format!("-{}", sh));
    }
    for lo in &longs {
        parts.push(format!("--{}", lo));
    }
    // NegBool: also surface `--no-NAME` for the canonical long name (if any).
    if matches!(s.kind, ArgKind::NegBool) {
        if let Some(lo) = longs.first() {
            parts.push(format!("--no-{}", lo));
        }
    }
    let names_str = parts.join(", ");
    let metavar = default_metavar(s.kind, &s.metavar);
    match s.kind {
        ArgKind::Bool | ArgKind::NegBool | ArgKind::Counter => names_str,
        _ if metavar.is_empty() => names_str,
        _ => format!("{} {}", names_str, metavar),
    }
}

fn build_help_text(specs: &[OptSpec], meta: &HelpMeta, include_help_row: bool) -> String {
    let prog = meta.prog.clone().unwrap_or_else(|| {
        std::env::args()
            .next()
            .as_deref()
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .unwrap_or_default()
    });
    let mut out = String::new();
    if prog.is_empty() {
        out.push_str("Usage: [OPTIONS]\n");
    } else {
        out.push_str(&format!("Usage: {} [OPTIONS]\n", prog));
    }
    if let Some(d) = &meta.desc {
        out.push('\n');
        out.push_str(d);
        out.push('\n');
    }
    out.push('\n');
    out.push_str("Options:\n");

    let mut rows: Vec<(String, String)> = Vec::new();
    for s in specs {
        let left = format_left_col(s);
        let mut right = s.help.clone().unwrap_or_default();
        let mut extras: Vec<String> = Vec::new();
        if let Some(d) = &s.default {
            extras.push(format!("default: {}", d));
        }
        if s.required {
            extras.push("required".to_string());
        }
        if !extras.is_empty() {
            if right.is_empty() {
                right = format!("({})", extras.join(", "));
            } else {
                right = format!("{} ({})", right, extras.join(", "));
            }
        }
        rows.push((left, right));
    }
    if include_help_row {
        rows.push((
            "-h, --help".to_string(),
            "show this help and exit".to_string(),
        ));
    }
    let max_left = rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
    for (l, r) in &rows {
        let pad = max_left.saturating_sub(l.chars().count());
        out.push_str(&format!("  {}{}  {}\n", l, " ".repeat(pad), r));
    }
    if let Some(e) = &meta.epilog {
        out.push('\n');
        out.push_str(e);
        out.push('\n');
    }
    out
}

fn argv_requests_help(argv: &[StrykeValue]) -> bool {
    for v in argv {
        let s = v.to_string();
        if s == "--" {
            return false;
        }
        if s == "--help" || s == "-h" {
            return true;
        }
    }
    false
}

/// True when every key in the hash is one of `prog`/`desc`/`description`/
/// `epilog` (or the hash is empty). Used to distinguish a META hashref from
/// a SPECS hashref at call sites that allow both.
fn hash_is_meta_shaped(h: &Arc<RwLock<IndexMap<String, StrykeValue>>>) -> bool {
    let g = h.read();
    g.iter()
        .all(|(k, _)| matches!(k.as_str(), "prog" | "desc" | "description" | "epilog"))
}

fn parse_specs_value(specs_val: &StrykeValue, line: usize) -> PerlResult<Vec<OptSpec>> {
    let mut specs: Vec<OptSpec> = Vec::new();
    if let Some(arr) = specs_val.as_array_ref() {
        for item in arr.read().iter() {
            let s = item.to_string();
            specs.push(parse_spec_string(&s, line)?);
        }
    } else if let Some(h) = specs_val.as_hash_ref() {
        for (k, v) in h.read().iter() {
            let mut s = parse_spec_string(k, line)?;
            if let Some(meta_ref) = v.as_hash_ref() {
                apply_metadata(&mut s, &meta_ref, line)?;
            } else {
                s.default = Some(v.clone());
            }
            specs.push(s);
        }
    } else if let Some(arr) = specs_val.as_array_vec() {
        for item in arr.iter() {
            let s = item.to_string();
            specs.push(parse_spec_string(&s, line)?);
        }
    } else {
        return Err(PerlError::runtime(
            "getopts: SPECS must be an array ref of spec strings or a hash ref",
            line,
        ));
    }
    Ok(specs)
}

/// Storage handle for the input argv. Either a borrowed `\@ARGV` array ref
/// (mutated in place) or the interpreter's `@ARGV` resolved through scope.
enum ArgvSink<'a> {
    Ref(Arc<RwLock<Vec<StrykeValue>>>),
    Scope(&'a mut VMHelper),
}

impl<'a> ArgvSink<'a> {
    fn read(&self) -> Vec<StrykeValue> {
        match self {
            ArgvSink::Ref(r) => r.read().clone(),
            ArgvSink::Scope(interp) => interp.scope.get_array("ARGV"),
        }
    }
    fn write(&mut self, val: Vec<StrykeValue>) -> PerlResult<()> {
        match self {
            ArgvSink::Ref(r) => {
                *r.write() = val;
                Ok(())
            }
            ArgvSink::Scope(interp) => interp.scope.set_array("ARGV", val),
        }
    }
}

/// Entry point. Signature (all forms accepted; `@ARGV` is mutated implicitly
/// when no explicit array ref is passed):
///
/// ```text
/// getopts(SPECS)                      # operate on @ARGV
/// getopts(SPECS, META)                # @ARGV + help-banner customization
/// getopts(\@ARGV, SPECS)              # explicit argv
/// getopts(\@ARGV, SPECS, META)        # explicit argv + meta
/// ```
///
/// `SPECS` is one of:
///   - an arrayref of spec strings: `[ "verbose|v", "file|f=s" ]`,
///   - a hashref of `{ spec => default-value }`, or
///   - a hashref of `{ spec => { help => ..., default => ..., required => ...,
///     metavar => ... } }` (D1 — metadata form).
///
/// `META` is a hashref with any subset of `{ prog, desc, epilog }`
/// controlling the auto-help banner. The (SPECS, META) two-arg form is
/// disambiguated from (ARGV_REF, SPECS) by inspecting the second arg's keys:
/// a hashref whose keys are all `prog`/`desc`/`description`/`epilog` (or
/// empty) is META; anything else is SPECS.
///
/// When any spec carries `help` text and the user hasn't defined their own
/// `--help`/`-h` option, `getopts` intercepts `--help`/`-h` in the input,
/// prints a formatted usage block to stdout, and calls `exit(0)`.
pub fn builtin_getopts(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> PerlResult<StrykeValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "getopts: usage: getopts(SPECS) | getopts(SPECS, META) | getopts(\\@ARGV, SPECS [, META])",
            line,
        ));
    }

    // Disambiguate the call shape.
    //
    //   1 arg:  SPECS — implicit @ARGV
    //   2 args: if args[1] is HashRef with only meta keys → (SPECS, META);
    //           else → (ARGV_REF, SPECS)
    //   3 args: (ARGV_REF, SPECS, META)
    let (argv_is_explicit, specs_idx, meta_idx): (bool, usize, Option<usize>) = match args.len() {
        1 => (false, 0, None),
        2 => {
            let arg1_is_meta = args[1]
                .as_hash_ref()
                .map(|h| hash_is_meta_shaped(&h))
                .unwrap_or(false);
            if arg1_is_meta {
                (false, 0, Some(1))
            } else {
                (true, 1, None)
            }
        }
        _ => (true, 1, Some(2)),
    };

    let mut argv_sink: ArgvSink<'_> = if argv_is_explicit {
        let r = args[0].as_array_ref().ok_or_else(|| {
            PerlError::runtime(
                "getopts: first argument (when 2+ args supplied) must be an array reference (\\@ARGV)",
                line,
            )
        })?;
        ArgvSink::Ref(r)
    } else {
        ArgvSink::Scope(interp)
    };

    let specs = parse_specs_value(&args[specs_idx], line)?;
    let help_meta = match meta_idx {
        Some(i) => parse_help_meta(&args[i], line)?,
        None => HelpMeta::default(),
    };

    // Detect duplicate canonical names.
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

    let user_owns_help = specs
        .iter()
        .any(|s| s.names().any(|n| n == "help" || n == "h"));
    let any_help_text = specs.iter().any(|s| s.help.is_some());
    let auto_help = any_help_text && !user_owns_help;

    let argv_snapshot = argv_sink.read();
    if auto_help && argv_requests_help(&argv_snapshot) {
        let text = build_help_text(&specs, &help_meta, true);
        print!("{}", text);
        std::process::exit(0);
    }

    let mut argv_copy = argv_snapshot;
    let result = parse_argv(&mut argv_copy, &specs, line)?;
    argv_sink.write(argv_copy)?;

    // Enforce `required`.
    for s in &specs {
        if !s.required {
            continue;
        }
        let present = match result.get(&s.canonical) {
            Some(v) => match s.kind {
                ArgKind::Required(_) | ArgKind::Optional(_) => true,
                ArgKind::Array(_) => v
                    .as_array_ref()
                    .map(|r| !r.read().is_empty())
                    .unwrap_or(false),
                ArgKind::Hash(_) => v
                    .as_hash_ref()
                    .map(|r| !r.read().is_empty())
                    .unwrap_or(false),
                ArgKind::Bool | ArgKind::NegBool | ArgKind::Counter => v.to_int() != 0,
            },
            None => false,
        };
        if !present {
            return Err(PerlError::runtime(
                format!("getopts: option '--{}' is required", s.canonical),
                line,
            ));
        }
    }

    Ok(StrykeValue::hash_ref(Arc::new(RwLock::new(result))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> StrykeValue {
        let v: Vec<StrykeValue> = items.iter().map(|s| StrykeValue::string((*s).into())).collect();
        StrykeValue::array_ref(Arc::new(RwLock::new(v)))
    }

    fn specs(items: &[&str]) -> StrykeValue {
        let v: Vec<StrykeValue> = items.iter().map(|s| StrykeValue::string((*s).into())).collect();
        StrykeValue::array_ref(Arc::new(RwLock::new(v)))
    }

    fn hget(v: &StrykeValue, k: &str) -> StrykeValue {
        v.as_hash_ref()
            .expect("hash ref result")
            .read()
            .get(k)
            .cloned()
            .unwrap_or(StrykeValue::UNDEF)
    }

    fn leftover(argv: &StrykeValue) -> Vec<String> {
        argv.as_array_ref()
            .expect("argv ref")
            .read()
            .iter()
            .map(|v| v.to_string())
            .collect()
    }

    /// Test wrapper: spin up a throwaway VMHelper and dispatch.
    fn call(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
        let mut interp = VMHelper::new();
        builtin_getopts(&mut interp, args, 1)
    }

    /// Variant that seeds `@ARGV` in the interpreter, runs getopts in the
    /// implicit-ARGV form, then returns `(result, leftover-@ARGV)`.
    fn call_with_argv(
        argv: &[&str],
        args: &[StrykeValue],
    ) -> PerlResult<(StrykeValue, Vec<String>)> {
        let mut interp = VMHelper::new();
        interp
            .scope
            .set_array(
                "ARGV",
                argv.iter().map(|s| StrykeValue::string((*s).into())).collect(),
            )
            .expect("seed @ARGV");
        let out = builtin_getopts(&mut interp, args, 1)?;
        let left = interp
            .scope
            .get_array("ARGV")
            .iter()
            .map(|v| v.to_string())
            .collect();
        Ok((out, left))
    }

    #[test]
    fn bool_flag_long_and_short() {
        let a = argv(&["--verbose", "rest"]);
        let s = specs(&["verbose|v"]);
        let out = call(&[a.clone(), s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(leftover(&a), vec!["rest".to_string()]);

        let a = argv(&["-v", "rest"]);
        let s = specs(&["verbose|v"]);
        let out = call(&[a.clone(), s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(leftover(&a), vec!["rest".to_string()]);
    }

    #[test]
    fn bool_default_zero_when_absent() {
        let a = argv(&["foo"]);
        let s = specs(&["verbose|v"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 0);
    }

    #[test]
    fn required_string_arg_separated_and_inline() {
        let a = argv(&["--file", "x.txt", "pos"]);
        let s = specs(&["file|f=s"]);
        let out = call(&[a.clone(), s]).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
        assert_eq!(leftover(&a), vec!["pos".to_string()]);

        let a = argv(&["--file=y.txt"]);
        let s = specs(&["file|f=s"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "y.txt");

        let a = argv(&["-fz.txt"]);
        let s = specs(&["file|f=s"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "z.txt");
    }

    #[test]
    fn int_and_float_coercion() {
        let a = argv(&["--count", "42", "--rate=3.14"]);
        let s = specs(&["count|n=i", "rate=f"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "count").to_int(), 42);
        assert!((hget(&out, "rate").to_number() - 3.14).abs() < 1e-9);
    }

    #[test]
    fn int_bad_value_errors() {
        let a = argv(&["--count", "abc"]);
        let s = specs(&["count=i"]);
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("expects integer"), "msg was: {}", msg);
    }

    #[test]
    fn array_repeatable() {
        let a = argv(&["-t", "a", "--tag=b", "-t", "c"]);
        let s = specs(&["tag|t=s@"]);
        let out = call(&[a, s]).unwrap();
        let v = hget(&out, "tag");
        let arr = v.as_array_ref().expect("arrayref").read().clone();
        let strs: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
        assert_eq!(strs, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn hash_kv() {
        let a = argv(&["-D", "k1=v1", "--define=k2=v2"]);
        let s = specs(&["define|D=s%"]);
        let out = call(&[a, s]).unwrap();
        let v = hget(&out, "define");
        let h = v.as_hash_ref().expect("hashref").read().clone();
        assert_eq!(h.get("k1").map(|v| v.to_string()).as_deref(), Some("v1"));
        assert_eq!(h.get("k2").map(|v| v.to_string()).as_deref(), Some("v2"));
    }

    #[test]
    fn counter_plus() {
        let a = argv(&["--debug", "--debug", "--debug"]);
        let s = specs(&["debug+"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "debug").to_int(), 3);
    }

    #[test]
    fn negatable_bool() {
        let a = argv(&["--no-color"]);
        let s = specs(&["color!"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "color").to_int(), 0);

        let a = argv(&["--color"]);
        let s = specs(&["color!"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "color").to_int(), 1);
    }

    #[test]
    fn double_dash_terminator() {
        let a = argv(&["--verbose", "--", "--not-an-option", "pos"]);
        let s = specs(&["verbose"]);
        let out = call(&[a.clone(), s]).unwrap();
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
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown option --nope"), "msg was: {}", msg);
    }

    #[test]
    fn missing_required_value_errors() {
        let a = argv(&["--file"]);
        let s = specs(&["file=s"]);
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("requires a value"), "msg was: {}", msg);
    }

    #[test]
    fn optional_value_absent_uses_zero() {
        let a = argv(&["--out", "--other"]);
        let s = specs(&["out:s", "other"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "out").to_string(), "");
        assert_eq!(hget(&out, "other").to_int(), 1);
    }

    #[test]
    fn defaults_via_hash_form() {
        let h: IndexMap<String, StrykeValue> = [
            ("count|n=i".to_string(), StrykeValue::integer(10)),
            ("file|f=s".to_string(), StrykeValue::string("out.txt".into())),
        ]
        .into_iter()
        .collect();
        let spec = StrykeValue::hash_ref(Arc::new(RwLock::new(h)));
        let a = argv(&[]);
        let out = call(&[a, spec]).unwrap();
        assert_eq!(hget(&out, "count").to_int(), 10);
        assert_eq!(hget(&out, "file").to_string(), "out.txt");
    }

    #[test]
    fn positional_stops_parsing() {
        let a = argv(&["--verbose", "input.txt", "--not-an-option"]);
        let s = specs(&["verbose"]);
        let out = call(&[a.clone(), s]).unwrap();
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
        let out = call(&[a.clone(), s]).unwrap();
        let _ = out;
        assert_eq!(leftover(&a), vec!["-5".to_string(), "-3.14".to_string()]);
    }

    #[test]
    fn duplicate_spec_errors() {
        let a = argv(&[]);
        let s = specs(&["verbose|v", "verbose=s"]);
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("duplicate option name 'verbose'"), "msg was: {}", msg);
    }

    #[test]
    fn bundled_short_bools() {
        let a = argv(&["-vDR", "pos"]);
        let s = specs(&["verbose|v", "debug|D", "recursive|R"]);
        let out = call(&[a.clone(), s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "debug").to_int(), 1);
        assert_eq!(hget(&out, "recursive").to_int(), 1);
        assert_eq!(leftover(&a), vec!["pos".to_string()]);
    }

    #[test]
    fn bundled_short_with_trailing_arg_taking_inline() {
        let a = argv(&["-vfx.txt"]);
        let s = specs(&["verbose|v", "file|f=s"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
    }

    #[test]
    fn bundled_short_with_trailing_arg_taking_separated() {
        let a = argv(&["-vf", "x.txt"]);
        let s = specs(&["verbose|v", "file|f=s"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
    }

    #[test]
    fn bundled_counter_repeated_char() {
        let a = argv(&["-vvv"]);
        let s = specs(&["verbose|v+"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 3);
    }

    #[test]
    fn bundle_unknown_char_errors() {
        let a = argv(&["-vXv"]);
        let s = specs(&["verbose|v"]);
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown option -X"), "msg was: {}", msg);
    }

    // ── D1: hash-of-hashref metadata form ──────────────────────────────────

    fn meta_spec(pairs: &[(&str, &[(&str, StrykeValue)])]) -> StrykeValue {
        let outer: IndexMap<String, StrykeValue> = pairs
            .iter()
            .map(|(k, meta_pairs)| {
                let inner: IndexMap<String, StrykeValue> = meta_pairs
                    .iter()
                    .map(|(mk, mv)| ((*mk).to_string(), mv.clone()))
                    .collect();
                (
                    (*k).to_string(),
                    StrykeValue::hash_ref(Arc::new(RwLock::new(inner))),
                )
            })
            .collect();
        StrykeValue::hash_ref(Arc::new(RwLock::new(outer)))
    }

    #[test]
    fn d1_metadata_default_and_help() {
        let s = meta_spec(&[
            (
                "file|f=s",
                &[
                    ("help", StrykeValue::string("output path".into())),
                    ("default", StrykeValue::string("out.txt".into())),
                ],
            ),
            (
                "count|n=i",
                &[("default", StrykeValue::integer(10))],
            ),
        ]);
        let a = argv(&[]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "out.txt");
        assert_eq!(hget(&out, "count").to_int(), 10);
    }

    #[test]
    fn d1_required_flag_missing_errors() {
        let s = meta_spec(&[(
            "file|f=s",
            &[
                ("help", StrykeValue::string("output".into())),
                ("required", StrykeValue::integer(1)),
            ],
        )]);
        let a = argv(&[]);
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("'--file' is required"), "msg was: {}", msg);
    }

    #[test]
    fn d1_required_flag_satisfied_passes() {
        let s = meta_spec(&[(
            "file|f=s",
            &[
                ("help", StrykeValue::string("output".into())),
                ("required", StrykeValue::integer(1)),
            ],
        )]);
        let a = argv(&["--file", "x.txt"]);
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
    }

    #[test]
    fn d1_mixed_scalar_and_hashref_values() {
        // One entry uses bare-scalar default (legacy hash form); the other
        // uses hashref metadata (D1). They must coexist in one spec hash.
        let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
        h.insert("verbose|v".to_string(), StrykeValue::integer(0));
        let inner: IndexMap<String, StrykeValue> = [(
            "help".to_string(),
            StrykeValue::string("output path".into()),
        )]
        .into_iter()
        .collect();
        h.insert(
            "file|f=s".to_string(),
            StrykeValue::hash_ref(Arc::new(RwLock::new(inner))),
        );
        let spec = StrykeValue::hash_ref(Arc::new(RwLock::new(h)));
        let a = argv(&["--verbose", "--file=x.txt"]);
        let out = call(&[a, spec]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
    }

    #[test]
    fn d1_unknown_metadata_key_errors() {
        let s = meta_spec(&[(
            "file|f=s",
            &[("nope", StrykeValue::integer(1))],
        )]);
        let a = argv(&[]);
        let err = call(&[a, s]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown metadata key 'nope'"), "msg was: {}", msg);
    }

    #[test]
    fn help_text_format_basic() {
        let specs = [
            OptSpec {
                canonical: "verbose".into(),
                aliases: vec!["v".into()],
                kind: ArgKind::Bool,
                default: None,
                help: Some("enable verbose".into()),
                required: false,
                metavar: None,
            },
            OptSpec {
                canonical: "file".into(),
                aliases: vec!["f".into()],
                kind: ArgKind::Required(ScalarType::Str),
                default: Some(StrykeValue::string("out.txt".into())),
                help: Some("output path".into()),
                required: false,
                metavar: None,
            },
            OptSpec {
                canonical: "count".into(),
                aliases: vec!["n".into()],
                kind: ArgKind::Required(ScalarType::Int),
                default: None,
                help: Some("iterations".into()),
                required: true,
                metavar: None,
            },
        ];
        let meta = HelpMeta {
            prog: Some("myscript".into()),
            desc: Some("do a thing".into()),
            epilog: None,
        };
        let text = build_help_text(&specs, &meta, true);
        // Banner
        assert!(text.contains("Usage: myscript [OPTIONS]"), "text:\n{}", text);
        assert!(text.contains("do a thing"), "text:\n{}", text);
        // Each option appears with both names and metavar where applicable
        assert!(text.contains("-v, --verbose"), "text:\n{}", text);
        assert!(text.contains("-f, --file VALUE"), "text:\n{}", text);
        assert!(text.contains("-n, --count N"), "text:\n{}", text);
        // Annotations
        assert!(text.contains("default: out.txt"), "text:\n{}", text);
        assert!(text.contains("required"), "text:\n{}", text);
        // Auto-help row
        assert!(text.contains("-h, --help"), "text:\n{}", text);
    }

    #[test]
    fn help_text_negbool_shows_no_form() {
        let specs = [OptSpec {
            canonical: "color".into(),
            aliases: vec![],
            kind: ArgKind::NegBool,
            default: None,
            help: Some("colored output".into()),
            required: false,
            metavar: None,
        }];
        let text = build_help_text(&specs, &HelpMeta::default(), false);
        assert!(text.contains("--color, --no-color"), "text:\n{}", text);
    }

    #[test]
    fn argv_requests_help_detects_both_forms() {
        assert!(argv_requests_help(&[StrykeValue::string("--help".into())]));
        assert!(argv_requests_help(&[
            StrykeValue::string("-x".into()),
            StrykeValue::string("-h".into()),
        ]));
        assert!(!argv_requests_help(&[
            StrykeValue::string("--".into()),
            StrykeValue::string("--help".into()),
        ]));
        assert!(!argv_requests_help(&[StrykeValue::string("--other".into())]));
    }

    #[test]
    fn user_can_claim_own_help_spec() {
        // When the user defines their own --help spec, getopts must NOT
        // intercept --help and exit. The user's spec wins.
        let s = meta_spec(&[
            (
                "verbose|v",
                &[("help", StrykeValue::string("verbose mode".into()))],
            ),
            (
                "help",
                &[("help", StrykeValue::string("custom help handling".into()))],
            ),
        ]);
        let a = argv(&["--help"]);
        // Should NOT exit. The --help token should be treated as the user's
        // own bool flag, returning a normal hash result.
        let out = call(&[a, s]).unwrap();
        assert_eq!(hget(&out, "help").to_int(), 1);
    }

    #[test]
    fn meta_unknown_key_errors() {
        let s = specs(&["verbose"]);
        let mut m: IndexMap<String, StrykeValue> = IndexMap::new();
        m.insert("nope".into(), StrykeValue::string("x".into()));
        let meta = StrykeValue::hash_ref(Arc::new(RwLock::new(m)));
        let a = argv(&[]);
        let err = call(&[a, s, meta]).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unknown meta key 'nope'"), "msg was: {}", msg);
    }

    // ── Implicit @ARGV form: 1-arg and (SPECS, META) ───────────────────────

    #[test]
    fn implicit_argv_one_arg() {
        let s = specs(&["verbose|v", "file|f=s"]);
        let (out, left) = call_with_argv(
            &["--verbose", "--file=x.txt", "pos1"],
            &[s],
        )
        .unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(hget(&out, "file").to_string(), "x.txt");
        assert_eq!(left, vec!["pos1".to_string()]);
    }

    #[test]
    fn implicit_argv_with_meta_second_arg() {
        let s = specs(&["verbose|v"]);
        let mut m: IndexMap<String, StrykeValue> = IndexMap::new();
        m.insert("prog".into(), StrykeValue::string("demo".into()));
        let meta = StrykeValue::hash_ref(Arc::new(RwLock::new(m)));
        let (out, left) =
            call_with_argv(&["-v", "rest"], &[s, meta]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert_eq!(left, vec!["rest".to_string()]);
    }

    #[test]
    fn implicit_argv_empty_meta_still_means_meta() {
        // An empty hashref as 2nd arg has zero keys, all of which are meta —
        // so this is the (SPECS, META) form. @ARGV is implicit.
        let s = specs(&["verbose|v"]);
        let m: IndexMap<String, StrykeValue> = IndexMap::new();
        let meta = StrykeValue::hash_ref(Arc::new(RwLock::new(m)));
        let (out, left) = call_with_argv(&["-v"], &[s, meta]).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        assert!(left.is_empty());
    }

    #[test]
    fn explicit_argv_two_arg_form_still_works() {
        // 2 args where arg1 is an ArrayRef (not a meta hash) → explicit-argv
        // form. Should NOT touch the interpreter's @ARGV.
        let mut interp = VMHelper::new();
        interp
            .scope
            .set_array(
                "ARGV",
                vec![StrykeValue::string("should-be-untouched".into())],
            )
            .unwrap();
        let a = argv(&["--verbose"]);
        let s = specs(&["verbose|v"]);
        let out = builtin_getopts(&mut interp, &[a.clone(), s], 1).unwrap();
        assert_eq!(hget(&out, "verbose").to_int(), 1);
        // Explicit-argv leftover lives in `a`, not in the interp's @ARGV.
        assert_eq!(leftover(&a), Vec::<String>::new());
        let interp_argv = interp.scope.get_array("ARGV");
        assert_eq!(interp_argv.len(), 1);
        assert_eq!(interp_argv[0].to_string(), "should-be-untouched");
    }

    #[test]
    fn implicit_argv_hash_specs() {
        // SPECS as a hashref (D1 metadata form) with 1 arg → implicit @ARGV.
        let s = meta_spec(&[(
            "file|f=s",
            &[("help", StrykeValue::string("output".into()))],
        )]);
        let (out, _left) = call_with_argv(&["--file=y.txt"], &[s]).unwrap();
        assert_eq!(hget(&out, "file").to_string(), "y.txt");
    }
}
