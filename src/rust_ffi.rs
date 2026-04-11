//! Inline Rust FFI — `rust { ... }` blocks compiled to a cdylib on first run, dlopened,
//! and registered as Perl-callable subs.
//!
//! Flow (driven by [`compile_and_register`], invoked from the builtin fallback in
//! [`crate::builtins::try_builtin`]):
//!
//! 1. Base64-decode the block body produced by [`crate::rust_sugar::desugar_rust_blocks`].
//! 2. SHA-256 the body → `<hash>` cache key.
//! 3. If `~/.cache/perlrs/ffi/<hash>.(dylib|so)` exists, dlopen and register.
//! 4. Otherwise: spit the body into `~/.cache/perlrs/ffi/<hash>.rs` wrapped in a minimal
//!    crate stub, invoke `rustc --crate-type=cdylib --edition=2021 -O`, then dlopen.
//! 5. Scan the body source for `pub extern "C" fn NAME(args) -> ret` using a tiny tokenizer,
//!    match each signature against the enumerated v1 table below, and register one entry
//!    in the per-process [`FFI_REGISTRY`] for each.
//!
//! ## Supported signatures (v1)
//!
//! | rust signature                               | perl arg types   | return   |
//! |----------------------------------------------|------------------|----------|
//! | `fn() -> i64`                                | —                | integer  |
//! | `fn(i64) -> i64`                             | 1× integer       | integer  |
//! | `fn(i64, i64) -> i64`                        | 2× integer       | integer  |
//! | `fn(i64, i64, i64) -> i64`                   | 3× integer       | integer  |
//! | `fn(i64, i64, i64, i64) -> i64`              | 4× integer       | integer  |
//! | `fn() -> f64`                                | —                | float    |
//! | `fn(f64) -> f64`                             | 1× float         | float    |
//! | `fn(f64, f64) -> f64`                        | 2× float         | float    |
//! | `fn(f64, f64, f64) -> f64`                   | 3× float         | float    |
//! | `fn(*const c_char) -> i64`                   | 1× string        | integer  |
//! | `fn(*const c_char) -> *const c_char`         | 1× string        | string   |
//!
//! That is the whole v1 palette. It's deliberately tiny — it covers the crc32 / hashing /
//! numeric-kernel use cases that actually motivate inline FFI. Extending the table is one
//! function pointer type plus one match arm per entry. A future revision can drop in
//! libffi if the signature set grows past what hand-enumeration can handle.
//!
//! ## Requirements at runtime
//!
//! `rustc` must be on `PATH`. First-run compilation costs ~1-2 seconds; subsequent runs
//! hit the cache and pay only dlopen (~10 ms). A clear error is raised when `rustc` is
//! missing — the script should not silently fall back.
//!
//! ## Limitations
//!
//! - The cdylib runs with the calling process's privileges. Same trust model as `do FILE`.
//! - `*const c_char` return values are allocated inside the cdylib; they must live for the
//!   duration of the call (we copy them into Perl strings immediately). The user is
//!   responsible for not returning dangling pointers.
//! - The body must be self-contained Rust: no external crates via Cargo. Use `std` only.

use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

/// One registered FFI entry: a function signature kind + a raw symbol pointer.
/// The dylib handle is kept alive in [`FFI_REGISTRY::libs`] so `sym` stays valid.
#[derive(Clone)]
struct FfiEntry {
    sig: FfiSig,
    sym: usize, // raw `*const ()` boxed as usize for Send+Sync
}

// SAFETY: The underlying pointer is a function pointer in a shared library that is kept
// alive by [`FFI_REGISTRY::libs`] for the process lifetime. It is never freed, and calling
// the function is already `unsafe`; the `Send + Sync` obligation here is the usual dlopen
// pattern used by libloading, ffi-rs, etc.
unsafe impl Send for FfiEntry {}
unsafe impl Sync for FfiEntry {}

#[derive(Clone, Copy, Debug)]
enum FfiSig {
    I0,       // fn() -> i64
    I1,       // fn(i64) -> i64
    I2,       // fn(i64, i64) -> i64
    I3,       // fn(i64, i64, i64) -> i64
    I4,       // fn(i64, i64, i64, i64) -> i64
    F0,       // fn() -> f64
    F1,       // fn(f64) -> f64
    F2,       // fn(f64, f64) -> f64
    F3,       // fn(f64, f64, f64) -> f64
    StrToInt, // fn(*const c_char) -> i64
    StrToStr, // fn(*const c_char) -> *const c_char
}

impl FfiSig {
    fn arity(self) -> usize {
        match self {
            FfiSig::I0 | FfiSig::F0 => 0,
            FfiSig::I1 | FfiSig::F1 | FfiSig::StrToInt | FfiSig::StrToStr => 1,
            FfiSig::I2 | FfiSig::F2 => 2,
            FfiSig::I3 | FfiSig::F3 => 3,
            FfiSig::I4 => 4,
        }
    }
}

/// Global registry: function name → entry. Populated by [`compile_and_register`] during
/// BEGIN; looked up by [`try_call`] on every bareword `FuncCall`.
struct Registry {
    entries: std::collections::HashMap<String, FfiEntry>,
    /// Loaded shared libraries kept alive for the process lifetime.
    libs: Vec<LoadedLib>,
}

struct LoadedLib {
    path: PathBuf,
    // Raw dlopen handle; retained solely so the library's symbol tables stay mapped for
    // the lifetime of the process. We never call `dlclose` — symbols are resolved at
    // registration time and cached in `FfiEntry::sym`.
    #[allow(dead_code)]
    handle: usize,
}

// SAFETY: dlopen handles are opaque and only closed on process exit here. All access is
// through read-only lookups; no Rust-side mutation through the pointer.
unsafe impl Send for LoadedLib {}
unsafe impl Sync for LoadedLib {}

static FFI_REGISTRY: std::sync::OnceLock<Arc<Mutex<Registry>>> = std::sync::OnceLock::new();

fn registry() -> &'static Arc<Mutex<Registry>> {
    FFI_REGISTRY.get_or_init(|| {
        Arc::new(Mutex::new(Registry {
            entries: std::collections::HashMap::new(),
            libs: Vec::new(),
        }))
    })
}

/// Lookup hook: returns `Some(Ok(result))` if `name` is a registered FFI function,
/// `None` otherwise. Called from [`crate::builtins::try_builtin`]'s fallback arm.
pub fn try_call(name: &str, args: &[PerlValue], line: usize) -> Option<PerlResult<PerlValue>> {
    let entry = {
        let guard = registry().lock();
        guard.entries.get(name).cloned()?
    };
    Some(invoke(name, &entry, args, line))
}

/// Perl builtin `__perlrs_rust_compile(BASE64, LINE)` — invoked at BEGIN time by the code
/// produced by [`crate::rust_sugar::desugar_rust_blocks`]. Idempotent per body hash.
pub fn compile_and_register(body_b64: &str, line: usize) -> PerlResult<()> {
    use base64::Engine as _;
    let body = base64::engine::general_purpose::STANDARD
        .decode(body_b64)
        .map_err(|e| PerlError::runtime(format!("rust FFI: invalid base64 body: {}", e), line))?;
    let body = String::from_utf8(body)
        .map_err(|e| PerlError::runtime(format!("rust FFI: non-utf8 body: {}", e), line))?;

    // Hash the body (not the wrapped crate source): same body → same dylib across perlrs
    // versions unless we bump the wrapper template.
    let mut hasher = Sha256::new();
    hasher.update(WRAPPER_SALT);
    hasher.update(body.as_bytes());
    let hash = hex_short(&hasher.finalize());

    // Cache dir: `~/.cache/perlrs/ffi/<hash>.*`.
    let cache_dir = ffi_cache_dir().map_err(|e| PerlError::runtime(e, line))?;
    let lib_path = cache_dir.join(format!("lib{}{}", hash, dylib_ext()));

    // Compile if missing.
    if !lib_path.exists() {
        let src_path = cache_dir.join(format!("{}.rs", hash));
        let wrapped = wrap_crate_source(&body);
        fs::write(&src_path, &wrapped)
            .map_err(|e| PerlError::runtime(format!("rust FFI: write source: {}", e), line))?;
        invoke_rustc(&src_path, &lib_path, line)?;
    }

    // dlopen (or reuse existing handle if same path is already loaded).
    let handle = dlopen_lib(&lib_path, line)?;

    // Parse the body for `pub extern "C" fn NAME(args) -> ret` declarations and register
    // each one against the shared library's symbol table.
    let decls = parse_extern_fns(&body);
    if decls.is_empty() {
        return Err(PerlError::runtime(
            "rust FFI: no `pub extern \"C\" fn ...` declarations found in block — v1 requires \
             at least one exported function"
                .to_string(),
            line,
        ));
    }

    let mut reg = registry().lock();
    // Remember the library so its symbols stay valid.
    if !reg.libs.iter().any(|l| l.path == lib_path) {
        reg.libs.push(LoadedLib {
            path: lib_path.clone(),
            handle,
        });
    }
    for (name, sig) in decls {
        let sym = dlsym_lookup(handle, &name, line)?;
        reg.entries.insert(
            name.clone(),
            FfiEntry {
                sig,
                sym: sym as usize,
            },
        );
    }
    Ok(())
}

fn ffi_cache_dir() -> Result<PathBuf, String> {
    let base = if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg)
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache")
    } else {
        return Err("rust FFI: cannot locate cache directory (no $HOME)".to_string());
    };
    let dir = base.join("perlrs").join("ffi");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("rust FFI: create cache dir {}: {}", dir.display(), e))?;
    Ok(dir)
}

fn hex_short(bytes: &[u8]) -> String {
    // 20 hex chars (10 bytes) is already 80 bits of namespace — plenty for a per-user cache.
    let mut s = String::with_capacity(20);
    for b in bytes.iter().take(10) {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn dylib_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        ".dylib"
    } else if cfg!(target_os = "windows") {
        ".dll"
    } else {
        ".so"
    }
}

/// Wrapper crate template. Keep this synchronized with [`WRAPPER_SALT`] — any change to
/// the template must bump the salt so stale cached dylibs are rebuilt.
const WRAPPER_SALT: &[u8] = b"perlrs-rust-ffi-v1";

fn wrap_crate_source(body: &str) -> String {
    // The user writes `pub extern "C" fn ...` declarations directly. We auto-insert
    // `#[no_mangle]` before each one so the compiled cdylib exports resolvable symbols
    // — without `no_mangle`, rustc adds a hash-suffix mangling and `dlsym(name)` fails.
    // The `cdylib` attribute lets us call `rustc` without `--crate-type=cdylib` on the CLI.
    let body = auto_no_mangle(body);
    format!(
        "// auto-generated by perlrs rust FFI\n\
         #![crate_type = \"cdylib\"]\n\
         #![allow(unused)]\n\
         #![allow(unused_imports)]\n\
         use std::os::raw::c_char;\n\
         use std::ffi::{{CStr, CString}};\n\
         \n\
         {body}\n"
    )
}

/// Insert `#[no_mangle]` before every `pub extern "C" fn` that does not already carry it.
/// Single left-to-right pass; preserves whitespace / indentation.
fn auto_no_mangle(body: &str) -> String {
    let needle = "pub extern \"C\" fn ";
    let mut out = String::with_capacity(body.len() + 32);
    let mut cursor = 0usize;
    while let Some(rel) = body[cursor..].find(needle) {
        let pos = cursor + rel;
        // Copy everything before the match.
        out.push_str(&body[cursor..pos]);
        // Walk backwards from `pos` past spaces/tabs to see if the previous non-space
        // chunk on the prior line is already `#[no_mangle]`.
        let line_start = body[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let indent = &body[line_start..pos];
        let already_marked = {
            let prev = body[..line_start].trim_end();
            prev.ends_with("#[no_mangle]")
        };
        if !already_marked {
            out.push_str("#[no_mangle]\n");
            out.push_str(indent);
        }
        // Emit the needle itself and resume scanning past it.
        out.push_str(needle);
        cursor = pos + needle.len();
    }
    out.push_str(&body[cursor..]);
    out
}

fn invoke_rustc(src: &PathBuf, out: &PathBuf, line: usize) -> PerlResult<()> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let status = std::process::Command::new(&rustc)
        .arg("--edition=2021")
        .arg("-O")
        .arg("-o")
        .arg(out)
        .arg(src)
        .output();
    let out_res = match status {
        Ok(o) => o,
        Err(e) => {
            return Err(PerlError::runtime(
                format!(
                    "rust FFI: failed to invoke `{}`: {}. Install Rust to use rust {{}} blocks.",
                    rustc, e
                ),
                line,
            ))
        }
    };
    if !out_res.status.success() {
        let stderr = String::from_utf8_lossy(&out_res.stderr);
        return Err(PerlError::runtime(
            format!(
                "rust FFI: rustc failed compiling {}:\n{}",
                src.display(),
                stderr
            ),
            line,
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn dlopen_lib(path: &Path, line: usize) -> PerlResult<usize> {
    use std::ffi::CString;
    let cpath = CString::new(path.to_string_lossy().as_bytes())
        .map_err(|e| PerlError::runtime(format!("rust FFI: dlopen path nul: {}", e), line))?;
    // SAFETY: libc::dlopen with RTLD_NOW|RTLD_LOCAL is the standard portable load path.
    let handle = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        // SAFETY: dlerror returns a static thread-local C string.
        let err = unsafe {
            let e = libc::dlerror();
            if e.is_null() {
                "unknown dlopen error".to_string()
            } else {
                CStr::from_ptr(e).to_string_lossy().into_owned()
            }
        };
        return Err(PerlError::runtime(
            format!("rust FFI: dlopen {}: {}", path.display(), err),
            line,
        ));
    }
    Ok(handle as usize)
}

#[cfg(not(unix))]
fn dlopen_lib(_path: &Path, line: usize) -> PerlResult<usize> {
    Err(PerlError::runtime(
        "rust FFI: only unix (Linux/macOS) is supported in v1".to_string(),
        line,
    ))
}

#[cfg(unix)]
fn dlsym_lookup(handle: usize, name: &str, line: usize) -> PerlResult<*const ()> {
    let cname = CString::new(name)
        .map_err(|e| PerlError::runtime(format!("rust FFI: symbol nul: {}", e), line))?;
    // SAFETY: handle came from a successful dlopen; dlsym returns a function pointer or NULL.
    let sym = unsafe { libc::dlsym(handle as *mut libc::c_void, cname.as_ptr()) };
    if sym.is_null() {
        return Err(PerlError::runtime(
            format!("rust FFI: symbol `{}` not found in compiled cdylib", name),
            line,
        ));
    }
    Ok(sym as *const ())
}

#[cfg(not(unix))]
fn dlsym_lookup(_h: usize, _n: &str, line: usize) -> PerlResult<*const ()> {
    Err(PerlError::runtime(
        "rust FFI: only unix supported in v1".to_string(),
        line,
    ))
}

/// Parse a Rust body for `pub extern "C" fn NAME(ARGS) -> RET` declarations that match one
/// of the v1 signatures. Declarations that do not match are silently ignored (they remain
/// inside the cdylib but are not Perl-callable) so users can freely write private helpers.
fn parse_extern_fns(body: &str) -> Vec<(String, FfiSig)> {
    let mut out = Vec::new();
    let needle = "pub extern \"C\" fn ";
    let mut start = 0usize;
    while let Some(rel) = body[start..].find(needle) {
        let pos = start + rel;
        let after = pos + needle.len();
        // Name: identifier characters.
        let bytes = body.as_bytes();
        let mut j = after;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
            j += 1;
        }
        if j == after {
            start = after;
            continue;
        }
        let name = body[after..j].to_string();
        // Skip whitespace to `(`.
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'(' {
            start = after;
            continue;
        }
        // Collect args until balanced `)`.
        let args_start = j + 1;
        let mut depth = 1i32;
        j += 1;
        while j < bytes.len() && depth > 0 {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let args_text = body[args_start..j].trim().to_string();
        j += 1; // past `)`
                // Optional `-> ret`.
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        let mut ret = String::new();
        if j + 1 < bytes.len() && bytes[j] == b'-' && bytes[j + 1] == b'>' {
            j += 2;
            while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                j += 1;
            }
            let rstart = j;
            while j < bytes.len()
                && bytes[j] != b'{'
                && bytes[j] != b';'
                && !(bytes[j] == b'w' && body[j..].starts_with("where"))
            {
                j += 1;
            }
            ret = body[rstart..j].trim().to_string();
        }
        if let Some(sig) = match_signature(&args_text, &ret) {
            out.push((name, sig));
        }
        start = j;
    }
    out
}

/// Match `(args)` + `-> ret` text against the v1 signature table. Args are expected to be
/// a comma-separated list with each element of the form `_name: TYPE`; we ignore the name.
fn match_signature(args_text: &str, ret: &str) -> Option<FfiSig> {
    let ret_norm: String = ret.split_whitespace().collect();
    let types: Vec<String> = if args_text.trim().is_empty() {
        Vec::new()
    } else {
        args_text
            .split(',')
            .map(|seg| {
                let seg = seg.trim();
                if let Some(colon) = seg.find(':') {
                    seg[colon + 1..].split_whitespace().collect::<String>()
                } else {
                    seg.split_whitespace().collect::<String>()
                }
            })
            .collect()
    };

    let all_i64 = !types.is_empty() && types.iter().all(|t| t == "i64");
    let all_f64 = !types.is_empty() && types.iter().all(|t| t == "f64");

    match (types.as_slice(), ret_norm.as_str()) {
        ([], "i64") => Some(FfiSig::I0),
        (_, "i64") if all_i64 && types.len() == 1 => Some(FfiSig::I1),
        (_, "i64") if all_i64 && types.len() == 2 => Some(FfiSig::I2),
        (_, "i64") if all_i64 && types.len() == 3 => Some(FfiSig::I3),
        (_, "i64") if all_i64 && types.len() == 4 => Some(FfiSig::I4),
        ([], "f64") => Some(FfiSig::F0),
        (_, "f64") if all_f64 && types.len() == 1 => Some(FfiSig::F1),
        (_, "f64") if all_f64 && types.len() == 2 => Some(FfiSig::F2),
        (_, "f64") if all_f64 && types.len() == 3 => Some(FfiSig::F3),
        _ => {
            // String-taking variants.
            if types.len() == 1 && is_c_str_ptr(&types[0]) {
                if ret_norm == "i64" {
                    return Some(FfiSig::StrToInt);
                }
                if is_c_str_ptr(&ret_norm) {
                    return Some(FfiSig::StrToStr);
                }
            }
            None
        }
    }
}

fn is_c_str_ptr(t: &str) -> bool {
    t == "*constc_char" || t == "*mutc_char"
}

fn invoke(name: &str, entry: &FfiEntry, args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    let expected = entry.sig.arity();
    if args.len() != expected {
        return Err(PerlError::runtime(
            format!(
                "rust FFI: {} expects {} args, got {}",
                name,
                expected,
                args.len()
            ),
            line,
        ));
    }
    // Each match arm transmutes the raw sym to the exact function-pointer type, then calls.
    // SAFETY: `sig` came from [`parse_extern_fns`], which only produces entries whose body
    // signature matches the arm type; `sym` is a valid function pointer into a dlopened
    // cdylib that stays alive for the process lifetime (see `FFI_REGISTRY::libs`).
    unsafe {
        match entry.sig {
            FfiSig::I0 => {
                let f: extern "C" fn() -> i64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::integer(f()))
            }
            FfiSig::I1 => {
                let f: extern "C" fn(i64) -> i64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::integer(f(args[0].to_int())))
            }
            FfiSig::I2 => {
                let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::integer(f(args[0].to_int(), args[1].to_int())))
            }
            FfiSig::I3 => {
                let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::integer(f(
                    args[0].to_int(),
                    args[1].to_int(),
                    args[2].to_int(),
                )))
            }
            FfiSig::I4 => {
                let f: extern "C" fn(i64, i64, i64, i64) -> i64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::integer(f(
                    args[0].to_int(),
                    args[1].to_int(),
                    args[2].to_int(),
                    args[3].to_int(),
                )))
            }
            FfiSig::F0 => {
                let f: extern "C" fn() -> f64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::float(f()))
            }
            FfiSig::F1 => {
                let f: extern "C" fn(f64) -> f64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::float(f(args[0].to_number())))
            }
            FfiSig::F2 => {
                let f: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::float(f(
                    args[0].to_number(),
                    args[1].to_number(),
                )))
            }
            FfiSig::F3 => {
                let f: extern "C" fn(f64, f64, f64) -> f64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::float(f(
                    args[0].to_number(),
                    args[1].to_number(),
                    args[2].to_number(),
                )))
            }
            FfiSig::StrToInt => {
                let s = args[0].to_string();
                let c = CString::new(s)
                    .map_err(|e| PerlError::runtime(format!("rust FFI: arg nul: {}", e), line))?;
                let f: extern "C" fn(*const c_char) -> i64 = std::mem::transmute(entry.sym);
                Ok(PerlValue::integer(f(c.as_ptr())))
            }
            FfiSig::StrToStr => {
                let s = args[0].to_string();
                let c = CString::new(s)
                    .map_err(|e| PerlError::runtime(format!("rust FFI: arg nul: {}", e), line))?;
                let f: extern "C" fn(*const c_char) -> *const c_char =
                    std::mem::transmute(entry.sym);
                let ret = f(c.as_ptr());
                if ret.is_null() {
                    return Ok(PerlValue::UNDEF);
                }
                let cs = CStr::from_ptr(ret);
                Ok(PerlValue::string(cs.to_string_lossy().into_owned()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_match_i2() {
        assert!(matches!(
            match_signature("a: i64, b: i64", "i64"),
            Some(FfiSig::I2)
        ));
    }

    #[test]
    fn signature_match_i0() {
        assert!(matches!(match_signature("", "i64"), Some(FfiSig::I0)));
    }

    #[test]
    fn signature_match_f3() {
        assert!(matches!(
            match_signature("a: f64, b: f64, c: f64", "f64"),
            Some(FfiSig::F3)
        ));
    }

    #[test]
    fn signature_mixed_types_rejected() {
        assert!(match_signature("a: i64, b: f64", "i64").is_none());
    }

    #[test]
    fn signature_str_to_str() {
        assert!(matches!(
            match_signature("s: *const c_char", "*const c_char"),
            Some(FfiSig::StrToStr)
        ));
    }

    #[test]
    fn signature_str_to_int() {
        assert!(matches!(
            match_signature("s: *const c_char", "i64"),
            Some(FfiSig::StrToInt)
        ));
    }

    #[test]
    fn parse_extern_fns_picks_up_simple_add() {
        let body = "pub extern \"C\" fn add(a: i64, b: i64) -> i64 { a + b }";
        let decls = parse_extern_fns(body);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].0, "add");
        assert!(matches!(decls[0].1, FfiSig::I2));
    }

    #[test]
    fn parse_extern_fns_ignores_unsupported_signatures() {
        let body = "pub extern \"C\" fn mixed(a: i64, b: f64) -> i64 { 0 }";
        let decls = parse_extern_fns(body);
        assert_eq!(decls.len(), 0);
    }

    #[test]
    fn parse_extern_fns_picks_up_multiple() {
        let body = "\
            pub extern \"C\" fn a1() -> i64 { 1 }\n\
            pub extern \"C\" fn a2(x: f64, y: f64) -> f64 { x + y }\n\
            fn private_helper() {}\n\
        ";
        let decls = parse_extern_fns(body);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].0, "a1");
        assert_eq!(decls[1].0, "a2");
    }
}
