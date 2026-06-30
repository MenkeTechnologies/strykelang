//! Native ahead-of-time compilation: `stryke build OUT --native SCRIPT`.
//!
//! Unlike the source-trailer build in [`crate::aot`] (which zstd-embeds the
//! script source into a copy of the `stryke` binary and re-parses + re-runs it
//! on the full interpreter at startup), the native path lowers the program to a
//! [`fusevm::Chunk`] via [`crate::fusevm_native::lower_to_fusevm_aot`], compiles
//! that chunk to a relocatable Cranelift object with [`fusevm::aot::compile_object`],
//! and links the object against strykelang's runtime staticlib (`libstryke.a`)
//! into a standalone executable. Same design as zshrs/vimlrs/awkrs `--native`.
//!
//! Scope: strykelang is not a pure fusevm frontend (it has its own VM); only the
//! subset [`crate::fusevm_native`] lowers self-contained — arithmetic, strings,
//! scalars, slots, arrays/hashes, and `print`/`say`/`printf` — is AOT-eligible.
//! Programs using subs, closures, regex, builtins, or block map/grep/sort are
//! rejected with a clear message (use `stryke build` without `--native`).
//!
//! Link contract (from `fusevm::aot`): the object exports `fusevm_aot_entry`
//! (the native driver) plus the serialized chunk blob, and imports
//! `fusevm_aot_register_builtins` — which strykelang provides below — resolved
//! at link time from `libstryke.a`. A tiny C `main` calls
//! `fusevm_aot_run_embedded`, fusevm's runtime entry.

use std::fs;
use std::path::{Path, PathBuf};

/// Frontend runtime hook invoked by `fusevm::aot::fusevm_aot_run_embedded` at
/// startup of a native AOT binary: install strykelang's native-value Extended-op
/// handler and a fresh leaked host on the run VM (see
/// [`crate::fusevm_native::aot_register`]).
///
/// # Safety
/// `vm` is the live run VM passed by the fusevm runtime; borrowed only here.
#[no_mangle]
// FFI entry point registered with the AOT runtime by raw `extern "C"` fn
// pointer; the `# Safety` contract above governs the deref. Marking the fn
// `unsafe` would change its type and break the fusevm link contract.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn fusevm_aot_register_builtins(vm: *mut fusevm::VM) {
    // SAFETY: the fusevm runtime hands us the live run VM for this call.
    let vm = unsafe { &mut *vm };
    crate::fusevm_native::aot_register(vm);
}

/// Locate the strykelang runtime staticlib to link against.
/// `STRYKE_AOT_RUNTIME_LIB` overrides; otherwise look for `libstryke.a` beside
/// the running executable.
fn runtime_staticlib() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("STRYKE_AOT_RUNTIME_LIB") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(dir) = exe.parent() {
        let cand = dir.join("libstryke.a");
        if cand.exists() {
            return Ok(cand);
        }
    }
    Err("could not locate libstryke.a (set STRYKE_AOT_RUNTIME_LIB)".to_string())
}

/// `stryke build OUT --native SCRIPT`: AOT-compile `script_path` to native
/// machine code and link a standalone executable at `out_path`.
pub fn build_native(script_path: &Path, out_path: &Path) -> Result<PathBuf, String> {
    let source = fs::read_to_string(script_path).map_err(|e| {
        format!(
            "stryke build --native: cannot read {}: {e}",
            script_path.display()
        )
    })?;
    let script_name = script_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("script.stk")
        .to_string();

    // Parse + compile on strykelang's own pipeline, then lower to a fusevm chunk.
    let program =
        crate::parse_with_file(&source, &script_name).map_err(|e| format!("{e}"))?;
    let chunk = crate::compiler::Compiler::new()
        .with_source_file(script_name.clone())
        .compile_program(&program)
        .map_err(|e| format!("stryke build --native: compile error: {e:?}"))?;
    let fchunk = crate::fusevm_native::lower_to_fusevm_aot(&chunk)
        .map_err(|e| format!("stryke build --native: {e}"))?;

    let runtime_lib = runtime_staticlib().map_err(|e| format!("stryke build --native: {e}"))?;

    let obj = out_path.with_extension("o");
    fusevm::aot::compile_object(&fchunk, &obj)
        .map_err(|e| format!("stryke build --native: {e}"))?;

    let stub = out_path.with_extension("aot_main.c");
    fs::write(
        &stub,
        b"extern long fusevm_aot_run_embedded(void);\nint main(void){return (int)fusevm_aot_run_embedded();}\n" as &[u8],
    )
    .map_err(|e| format!("stryke build --native: write entry stub: {e}"))?;

    // The runtime staticlib loses the `#[link]` directives that rustc would
    // normally honor, so every native library strykelang's runtime depends on
    // must be named explicitly here. The set below covers strykelang's bundled
    // zsh port (ncurses/termcap), compression/encoding, and the macOS frameworks
    // its network/time deps pull. `STRYKE_AOT_LINK_ARGS` (whitespace-separated)
    // appends extra flags for environments that need more.
    let mut cmd = std::process::Command::new("cc");
    cmd.arg(&stub).arg(&obj).arg(&runtime_lib);
    if cfg!(target_os = "macos") {
        // Homebrew lib dirs for pcre2 (Apple Silicon + Intel prefixes).
        cmd.arg("-L/opt/homebrew/lib").arg("-L/usr/local/lib");
        // System / brew libraries strykelang's runtime needs: bundled zsh port
        // (ncurses/termcap), compression (z/bz2/lzma), encoding (iconv), regex
        // (pcre2), and the C++ runtime.
        for lib in ["-lncurses", "-lz", "-lbz2", "-llzma", "-liconv", "-lc++", "-lpcre2-8"] {
            cmd.arg(lib);
        }
        // Frameworks: CoreFoundation/Security/SystemConfiguration (net/time),
        // CoreServices (FSEvents file-watching), IOKit (hardware queries).
        for fw in [
            "CoreFoundation",
            "Security",
            "SystemConfiguration",
            "CoreServices",
            "IOKit",
        ] {
            cmd.arg("-framework").arg(fw);
        }
    } else {
        for lib in [
            "-lncurses", "-lz", "-lbz2", "-llzma", "-lm", "-lpthread", "-ldl",
        ] {
            cmd.arg(lib);
        }
    }
    // Escape hatch for environments needing extra flags (whitespace-separated).
    if let Ok(extra) = std::env::var("STRYKE_AOT_LINK_ARGS") {
        for a in extra.split_whitespace() {
            cmd.arg(a);
        }
    }
    cmd.arg("-o").arg(out_path);
    let status = cmd
        .status()
        .map_err(|e| format!("stryke build --native: invoking cc: {e}"))?;
    let _ = fs::remove_file(&stub);
    let _ = fs::remove_file(&obj);
    if !status.success() {
        return Err(format!(
            "stryke build --native: link failed (cc exit {:?}); the strykelang \
             runtime staticlib pulls many native libraries — see cc output above",
            status.code()
        ));
    }
    set_executable(out_path);
    Ok(out_path.to_path_buf())
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut p = meta.permissions();
        p.set_mode(p.mode() | 0o111);
        let _ = fs::set_permissions(path, p);
    }
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}
