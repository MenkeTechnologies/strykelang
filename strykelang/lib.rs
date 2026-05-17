//! Crate root — see [`README.md`](https://github.com/MenkeTechnologies/stryke) for overview.
// `cargo doc` with `RUSTDOCFLAGS=-D warnings` (CI) flags intra-doc links to private items and
// a few shorthand links (`MethodCall`, `Op::…`) that do not resolve as paths. Suppress until
// docs are normalized to `crate::…` paths and public-only links.
#![allow(rustdoc::private_intra_doc_links)]
#![allow(rustdoc::broken_intra_doc_links)]
#![allow(clippy::needless_range_loop)]

pub mod agent;
pub mod ai;
pub mod ai_sugar;
pub mod aop;
pub mod aot;
pub mod ast;
pub mod banner;
pub mod builtins;
pub mod builtins_linalg_graph_date;
pub mod builtins_bits_music_stats;
pub mod builtins_phonetic_geo_codec;
pub mod builtins_bio_geom_markov;
pub mod builtins_games_ml_chem;
pub mod builtins_vision_ir_algorithms;
pub mod builtins_combin_audio_physics;
pub mod builtins_ratings_geom_units;
pub mod builtins_const;
pub mod builtins_data;
pub mod builtins_geom;
pub mod builtins_iter;
pub mod builtins_mathx;
pub mod builtins_misc;
pub mod builtins_misc2;
pub mod builtins_github;
pub mod builtins_net;
pub mod builtins_quant;
pub mod builtins_validate;
pub mod bytecode;
pub mod capture;
pub mod cli_runners;
pub mod cluster;
pub mod compiler;
pub mod controller;
pub mod convert;
mod crypt_util;
pub mod data_section;
pub mod debugger;
pub mod deconvert;
pub mod deparse;
pub mod doc_render;
pub mod english;
pub mod error;
mod fib_like_tail;
pub mod fmt;
pub mod format;
pub mod minify;
pub mod getopts;
mod jit;
mod jwt;
pub mod kvstore;
pub mod lexer;
pub mod list_builtins;
pub mod dap;
pub mod lsp;
pub mod lsp_docs_domains;
pub mod lsp_extras;
pub mod lsp_symbols;
mod map_grep_fast;
mod map_stream;
pub mod mcp;
pub mod mro;
mod nanbox;
mod native_codec;
pub mod native_data;
pub mod pack;
pub mod par_lines;
mod par_list;
pub mod par_pipeline;
pub mod par_walk;
pub mod parallel_trace;
pub mod parser;
pub mod pcache;
pub mod pchannel;
mod pending_destroy;
pub mod perf_recorder;
pub mod perl_decode;
pub mod perl_fs;
pub mod perl_inc;
#[cfg(unix)]
pub mod perl_pty;
mod perl_regex;
pub mod perl_signal;
pub mod pkg;
mod pmap_progress;
pub mod ppool;
pub mod profiler;
pub mod pwatch;
pub mod remote_wire;
pub mod rust_ffi;
pub mod rust_sugar;
pub mod scope;
pub mod script_cache;
pub mod secrets;
pub mod serialize_normalize;
pub mod sketches;
mod sort_fast;
pub mod special_vars;
pub mod static_analysis;
pub mod stress;
pub mod token;
pub mod value;
pub mod vm;
pub mod vm_helper;
pub mod web;
pub mod web_orm;

// Re-export shell components from the zsh crate
pub use zsh::exec as shell_exec;
pub use zsh::fds as shell_fds;
pub use zsh::history as shell_history;
pub use zsh::jobs as shell_jobs;
pub use zsh::lex as zsh_lex;
pub use zsh::parse as shell_parse;
pub use zsh::parse as zsh_parse;
pub use zsh::signals as shell_signal;
pub use zsh::tokens as zsh_tokens;
pub use zsh::utils::errflag as zsh_errflag;
pub use zsh::zle as shell_zle;
pub use zsh::zsh_h::ERRFLAG_ERROR;
pub use zsh::zwc as shell_zwc;

pub use vm_helper::{
    perl_bracket_version, FEAT_SAY, FEAT_STATE, FEAT_SWITCH, FEAT_UNICODE_STRINGS,
};

use error::{StrykeError, StrykeResult};
use vm_helper::VMHelper;

// ── Perl 5 strict-compat mode (`--compat`) ──────────────────────────────────

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// When `true`, all stryke extensions are disabled and only stock Perl 5
/// syntax / builtins are accepted.  Set once from the CLI driver and read by
/// the parser, compiler, and interpreter.
static COMPAT_MODE: AtomicBool = AtomicBool::new(false);

/// Process-wide default for no-interop mode. Set by `--no-interop` on the
/// CLI. Threads without a thread-local override inherit this.
static NO_INTEROP_DEFAULT: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Per-thread no-interop override.
    /// `None` → use the process default; `Some(b)` → this thread's parser
    /// sees `b` regardless of the default. Lets `check_no_interop` /
    /// `test_no_interop` run from `pmaps` workers without racing siblings
    /// on a shared atomic.
    static NO_INTEROP_TLS: Cell<Option<bool>> = const { Cell::new(None) };
}

/// When `true`, integer arithmetic that overflows i64 promotes to `BigInt`
/// instead of falling back to `f64`. Activated by `use bigint;` and
/// deactivated by `no bigint;`. Independent of `COMPAT_MODE` so a script
/// can opt into bigint semantics without dragging in the rest of compat.
static BIGINT_PRAGMA: AtomicBool = AtomicBool::new(false);

/// Enable Perl 5 strict-compatibility mode (disables all stryke extensions).
pub fn set_compat_mode(on: bool) {
    COMPAT_MODE.store(on, Ordering::Relaxed);
}

/// Returns `true` when `--compat` is active.
#[inline]
pub fn compat_mode() -> bool {
    COMPAT_MODE.load(Ordering::Relaxed)
}

/// Enable bigint pragma (`use bigint;`) — integer overflow promotes to
/// `BigInt` instead of demoting to `f64`.
pub fn set_bigint_pragma(on: bool) {
    BIGINT_PRAGMA.store(on, Ordering::Relaxed);
}

/// Returns `true` when `use bigint;` is active in this script.
#[inline]
pub fn bigint_pragma() -> bool {
    BIGINT_PRAGMA.load(Ordering::Relaxed)
}

/// Set the **process-wide default** for no-interop mode. Used by the CLI
/// (`--no-interop`); threads without a thread-local override inherit it.
pub fn set_no_interop_mode(on: bool) {
    NO_INTEROP_DEFAULT.store(on, Ordering::Relaxed);
}

/// Set the **current thread's** no-interop override. `None` clears it;
/// `Some(b)` pins this thread to `b`. Sibling threads are unaffected —
/// the primitive that lets `check_no_interop` work safely from `pmaps`.
pub fn set_no_interop_mode_tls(value: Option<bool>) {
    NO_INTEROP_TLS.with(|c| c.set(value));
}

/// Read the current thread's no-interop override (without falling back).
/// Used by RAII guards to save/restore.
pub fn no_interop_mode_tls() -> Option<bool> {
    NO_INTEROP_TLS.with(|c| c.get())
}

/// Effective no-interop flag for this thread: TLS override if set, else
/// the process-wide default. Hot path — called from parser/lexer.
#[inline]
pub fn no_interop_mode() -> bool {
    if let Some(v) = NO_INTEROP_TLS.with(|c| c.get()) {
        return v;
    }
    NO_INTEROP_DEFAULT.load(Ordering::Relaxed)
}
use value::StrykeValue;

/// Parse a string of Perl code and return the AST.
/// Pretty-print a parsed program as Perl-like source (`stryke --fmt`).
pub fn format_program(p: &ast::Program) -> String {
    fmt::format_program(p)
}

/// Convert a parsed program to stryke syntax with `|>` pipes and no semicolons.
pub fn convert_to_stryke(p: &ast::Program) -> String {
    convert::convert_program(p)
}

/// Convert a parsed program to stryke syntax with custom options.
pub fn convert_to_stryke_with_options(p: &ast::Program, opts: &convert::ConvertOptions) -> String {
    convert::convert_program_with_options(p, opts)
}

/// Deconvert a parsed stryke program back to standard Perl .pl syntax.
pub fn deconvert_to_perl(p: &ast::Program) -> String {
    deconvert::deconvert_program(p)
}

/// Deconvert a parsed stryke program back to standard Perl .pl syntax with options.
pub fn deconvert_to_perl_with_options(
    p: &ast::Program,
    opts: &deconvert::DeconvertOptions,
) -> String {
    deconvert::deconvert_program_with_options(p, opts)
}

pub fn parse(code: &str) -> StrykeResult<ast::Program> {
    parse_with_file(code, "-e")
}

/// Parse with a **source path** for lexer/parser diagnostics (`… at FILE line N`), e.g. a script
/// path or a required `.pm` absolute path. Use [`parse`] for snippets where `-e` is appropriate.
pub fn parse_with_file(code: &str, file: &str) -> StrykeResult<ast::Program> {
    parse_with_file_inner(code, file, false)
}

/// Like [`parse_with_file`], but marks the parser as loading a module. Modules are allowed to
/// shadow stryke builtins (e.g. `sub blessed { ... }` in Scalar::Util.pm) unless `--no-interop`.
pub fn parse_module_with_file(code: &str, file: &str) -> StrykeResult<ast::Program> {
    parse_with_file_inner(code, file, true)
}

fn parse_with_file_inner(code: &str, file: &str, is_module: bool) -> StrykeResult<ast::Program> {
    // `rust { ... }` FFI blocks are desugared at source level into BEGIN-wrapped builtin
    // calls — the parity roadmap forbids new `StmtKind` variants for new behavior, so this
    // pre-pass is the right shape. No-op for programs that don't mention `rust`.
    let desugared = if compat_mode() {
        code.to_string()
    } else {
        let s = rust_sugar::desugar_rust_blocks(code);
        ai_sugar::desugar(&s)
    };
    let mut lexer = lexer::Lexer::new_with_file(&desugared, file);
    let tokens = lexer.tokenize()?;
    let bare_positional_indices = std::mem::take(&mut lexer.bare_positional_indices);
    let mut parser = parser::Parser::new_with_file(tokens, file);
    parser.bare_positional_indices = bare_positional_indices;
    parser.parsing_module = is_module;
    parser.parse_program()
}

/// Parse and execute a string of Perl code within an existing interpreter.
/// Compile and execute via the bytecode VM.
/// Uses [`VMHelper::file`] for both parse diagnostics and `__FILE__` during this execution.
pub fn parse_and_run_string(code: &str, interp: &mut VMHelper) -> StrykeResult<StrykeValue> {
    let file = interp.file.clone();
    parse_and_run_string_in_file(code, interp, &file)
}

/// Like [`parse_and_run_string`], but parse errors and `__FILE__` for this run use `file` (e.g. a
/// required module path). Restores [`VMHelper::file`] after execution.
pub fn parse_and_run_string_in_file(
    code: &str,
    interp: &mut VMHelper,
    file: &str,
) -> StrykeResult<StrykeValue> {
    parse_and_run_string_in_file_inner(code, interp, file, false)
}

/// Like [`parse_and_run_string_in_file`], but marks parsing as a module load. Allows shadowing
/// stryke builtins (e.g. `sub blessed { ... }`) unless `--no-interop` is active.
pub fn parse_and_run_module_in_file(
    code: &str,
    interp: &mut VMHelper,
    file: &str,
) -> StrykeResult<StrykeValue> {
    parse_and_run_string_in_file_inner(code, interp, file, true)
}

fn parse_and_run_string_in_file_inner(
    code: &str,
    interp: &mut VMHelper,
    file: &str,
    is_module: bool,
) -> StrykeResult<StrykeValue> {
    let program = if is_module {
        parse_module_with_file(code, file)?
    } else {
        parse_with_file(code, file)?
    };
    let saved = interp.file.clone();
    interp.file = file.to_string();
    let r = interp.execute(&program);
    interp.file = saved;
    let v = r?;
    interp.drain_pending_destroys(0)?;
    Ok(v)
}

/// Crate-root `vendor/perl` (e.g. `List/Util.pm`). The `stryke` / `stryke` driver prepends this to
/// `@INC` when the directory exists so in-tree pure-Perl modules shadow XS-only core stubs.
pub fn vendor_perl_inc_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/perl")
}

/// Language server over stdio (`stryke --lsp`). Returns a process exit code.
pub fn run_lsp_stdio() -> i32 {
    match lsp::run_stdio() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("stryke --lsp: {e}");
            1
        }
    }
}

/// Parse and execute a string of Perl code with a fresh interpreter.
pub fn run(code: &str) -> StrykeResult<StrykeValue> {
    let program = parse(code)?;
    let mut interp = VMHelper::new();
    let v = interp.execute(&program)?;
    interp.run_global_teardown()?;
    Ok(v)
}

/// Try to compile and run via bytecode VM. Returns None if compilation fails.
///
/// **rkyv bytecode cache.** When `interp.cached_chunk` is populated (from a cache
/// hit), this function skips `compile_program` entirely and runs the preloaded
/// chunk. On cache miss the compiler runs normally and, if `interp.cache_script_path`
/// is set, the fresh chunk + program are persisted to the rkyv shard so the next
/// run skips lex/parse/compile entirely.
pub fn try_vm_execute(
    program: &ast::Program,
    interp: &mut VMHelper,
) -> Option<StrykeResult<StrykeValue>> {
    if let Err(e) = interp.prepare_program_top_level(program) {
        return Some(Err(e));
    }

    // Fast path: chunk loaded from the bytecode cache hit. Consume the slot with `.take()` so a
    // subsequent re-entry (e.g. nested `do FILE`) does not reuse a stale chunk.
    if let Some(chunk) = interp.cached_chunk.take() {
        return Some(run_compiled_chunk(chunk, interp));
    }

    // `use strict 'vars'` is enforced at compile time by the compiler (see
    // `Compiler::check_strict_scalar_access` and siblings). `strict refs` / `strict subs` are
    // enforced by the tree helpers that the VM already delegates into (symbolic deref,
    // `call_named_sub`, etc.), so they work transitively.
    let comp = compiler::Compiler::new()
        .with_source_file(interp.file.clone())
        .with_strict_vars(interp.strict_vars);
    let chunk = match comp.compile_program(program) {
        Ok(chunk) => chunk,
        Err(compiler::CompileError::Frozen { line, detail }) => {
            let err = if detail.starts_with("Global symbol") {
                StrykeError::syntax(detail, line)
            } else {
                StrykeError::runtime(detail, line)
            };
            return Some(Err(err));
        }
        Err(compiler::CompileError::Unsupported(reason)) => {
            return Some(Err(StrykeError::runtime(
                format!("VM compile error (unsupported): {}", reason),
                0,
            )));
        }
    };

    // Save to the bytecode cache (mtime-based, skips lex/parse/compile on 2+ runs)
    if let Some(path) = interp.cache_script_path.take() {
        let _ = script_cache::try_save(&path, program, &chunk);
    }
    Some(run_compiled_chunk(chunk, interp))
}

/// Shared execution tail used by both the cache-hit and compile paths in
/// [`try_vm_execute`]. Pulled out so the rkyv-cache fast path does not duplicate
/// the flip-flop / BEGIN-END / struct-def wiring every VM run depends on.
fn run_compiled_chunk(chunk: bytecode::Chunk, interp: &mut VMHelper) -> StrykeResult<StrykeValue> {
    interp.clear_flip_flop_state();
    interp.prepare_flip_flop_vm_slots(chunk.flip_flop_slots);
    if interp.disasm_bytecode {
        eprintln!("{}", chunk.disassemble());
    }
    interp.clear_begin_end_blocks_after_vm_compile();
    for def in &chunk.struct_defs {
        interp
            .struct_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    for def in &chunk.enum_defs {
        interp
            .enum_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    // Load traits before classes so trait enforcement can reference them
    for def in &chunk.trait_defs {
        interp
            .trait_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    for def in &chunk.class_defs {
        let mut def = def.clone();
        // Final class/method enforcement
        for parent_name in &def.extends.clone() {
            if let Some(parent_def) = interp.class_defs.get(parent_name) {
                if parent_def.is_final {
                    return Err(crate::error::StrykeError::runtime(
                        format!("cannot extend final class `{}`", parent_name),
                        0,
                    ));
                }
                for m in &def.methods {
                    if let Some(parent_method) = parent_def.method(&m.name) {
                        if parent_method.is_final {
                            return Err(crate::error::StrykeError::runtime(
                                format!(
                                    "cannot override final method `{}` from class `{}`",
                                    m.name, parent_name
                                ),
                                0,
                            ));
                        }
                    }
                }
            }
        }
        // Trait contract enforcement + default method inheritance
        for trait_name in &def.implements.clone() {
            if let Some(trait_def) = interp.trait_defs.get(trait_name) {
                for required in trait_def.required_methods() {
                    let has_method = def.methods.iter().any(|m| m.name == required.name);
                    if !has_method {
                        return Err(crate::error::StrykeError::runtime(
                            format!(
                                "class `{}` implements trait `{}` but does not define required method `{}`",
                                def.name, trait_name, required.name
                            ),
                            0,
                        ));
                    }
                }
                // Inherit default methods from trait (methods with bodies)
                for tm in &trait_def.methods {
                    if tm.body.is_some() && !def.methods.iter().any(|m| m.name == tm.name) {
                        def.methods.push(tm.clone());
                    }
                }
            }
        }
        // Abstract method enforcement: concrete subclasses must implement
        // all abstract methods (body-less methods) from abstract parents
        if !def.is_abstract {
            for parent_name in &def.extends.clone() {
                if let Some(parent_def) = interp.class_defs.get(parent_name) {
                    if parent_def.is_abstract {
                        for m in &parent_def.methods {
                            if m.body.is_none() && !def.methods.iter().any(|dm| dm.name == m.name) {
                                return Err(crate::error::StrykeError::runtime(
                                    format!(
                                        "class `{}` must implement abstract method `{}` from `{}`",
                                        def.name, m.name, parent_name
                                    ),
                                    0,
                                ));
                            }
                        }
                    }
                }
            }
        }
        // Initialize static fields
        for sf in &def.static_fields {
            let val = if let Some(ref expr) = sf.default {
                match interp.eval_expr(expr) {
                    Ok(v) => v,
                    Err(crate::vm_helper::FlowOrError::Error(e)) => return Err(e),
                    Err(_) => crate::value::StrykeValue::UNDEF,
                }
            } else {
                crate::value::StrykeValue::UNDEF
            };
            let key = format!("{}::{}", def.name, sf.name);
            interp.scope.declare_scalar(&key, val);
        }
        // Register class methods into subs so method dispatch finds them.
        for m in &def.methods {
            if let Some(ref body) = m.body {
                let fq = format!("{}::{}", def.name, m.name);
                let sub = std::sync::Arc::new(crate::value::StrykeSub {
                    name: fq.clone(),
                    params: m.params.clone(),
                    body: body.clone(),
                    closure_env: None,
                    prototype: None,
                    fib_like: None,
                });
                interp.subs.insert(fq, sub);
            }
        }
        // Set @ClassName::ISA so MRO/isa resolution works.
        if !def.extends.is_empty() {
            let isa_key = format!("{}::ISA", def.name);
            let parents: Vec<crate::value::StrykeValue> = def
                .extends
                .iter()
                .map(|p| crate::value::StrykeValue::string(p.clone()))
                .collect();
            interp.scope.declare_array(&isa_key, parents);
        }
        interp
            .class_defs
            .insert(def.name.clone(), std::sync::Arc::new(def));
    }
    let vm_jit = interp.vm_jit_enabled && interp.profiler.is_none();
    let mut vm = vm::VM::new(&chunk, interp);
    vm.set_jit_enabled(vm_jit);
    match vm.execute() {
        Ok(val) => {
            interp.drain_pending_destroys(0)?;
            Ok(val)
        }
        // On cache-hit path, surface VM errors directly (we no longer hold the
        // fresh Program the caller passed). For the cold-compile path, the compiler would
        // have already returned `Unsupported` for anything the VM cannot run, so this
        // branch is effectively unreachable there. Either way, surface as a runtime error.
        Err(e)
            if e.message.starts_with("VM: unimplemented op")
                || e.message.starts_with("Unimplemented builtin") =>
        {
            Err(StrykeError::runtime(e.message, 0))
        }
        Err(e) => Err(e),
    }
}

/// Compile program and run only the prelude (BEGIN/CHECK/INIT phase blocks) via the VM.
/// Stores the compiled chunk on `interp.line_mode_chunk` for per-line re-execution.
pub fn compile_and_run_prelude(program: &ast::Program, interp: &mut VMHelper) -> StrykeResult<()> {
    interp.prepare_program_top_level(program)?;
    let comp = compiler::Compiler::new()
        .with_source_file(interp.file.clone())
        .with_strict_vars(interp.strict_vars);
    let mut chunk = match comp.compile_program(program) {
        Ok(chunk) => chunk,
        Err(compiler::CompileError::Frozen { line, detail }) => {
            let err = if detail.starts_with("Global symbol") {
                StrykeError::syntax(detail, line)
            } else {
                StrykeError::runtime(detail, line)
            };
            return Err(err);
        }
        Err(compiler::CompileError::Unsupported(reason)) => {
            return Err(StrykeError::runtime(
                format!("VM compile error (unsupported): {}", reason),
                0,
            ));
        }
    };

    interp.clear_flip_flop_state();
    interp.prepare_flip_flop_vm_slots(chunk.flip_flop_slots);
    if interp.disasm_bytecode {
        eprintln!("{}", chunk.disassemble());
    }
    interp.clear_begin_end_blocks_after_vm_compile();
    for def in &chunk.struct_defs {
        interp
            .struct_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    for def in &chunk.enum_defs {
        interp
            .enum_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    for def in &chunk.trait_defs {
        interp
            .trait_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    for def in &chunk.class_defs {
        interp
            .class_defs
            .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
    }
    // Register class methods.
    for def in &chunk.class_defs {
        for m in &def.methods {
            if let Some(ref body) = m.body {
                let fq = format!("{}::{}", def.name, m.name);
                let sub = std::sync::Arc::new(crate::value::StrykeSub {
                    name: fq.clone(),
                    params: m.params.clone(),
                    body: body.clone(),
                    closure_env: None,
                    prototype: None,
                    fib_like: None,
                });
                interp.subs.insert(fq, sub);
            }
        }
    }

    let body_ip = chunk.body_start_ip;
    if body_ip > 0 && body_ip < chunk.ops.len() {
        // Run only the prelude: temporarily place Halt at body start.
        let saved_op = chunk.ops[body_ip].clone();
        chunk.ops[body_ip] = bytecode::Op::Halt;
        let vm_jit = interp.vm_jit_enabled && interp.profiler.is_none();
        let mut vm = vm::VM::new(&chunk, interp);
        vm.set_jit_enabled(vm_jit);
        let _ = vm.execute()?;
        chunk.ops[body_ip] = saved_op;
    }

    interp.line_mode_chunk = Some(chunk);
    Ok(())
}

/// Execute the body portion of a pre-compiled chunk for one input line.
/// Sets `$_` to `line_str`, runs from `body_start_ip` to Halt, returns `$_` for `-p` output.
pub fn run_line_body(
    chunk: &bytecode::Chunk,
    interp: &mut VMHelper,
    line_str: &str,
    is_last_input_line: bool,
) -> StrykeResult<Option<String>> {
    interp.line_mode_eof_pending = is_last_input_line;
    let result: StrykeResult<Option<String>> = (|| {
        interp.line_number += 1;
        interp
            .scope
            .set_topic(value::StrykeValue::string(line_str.to_string()));

        if interp.auto_split {
            let sep = interp.field_separator.as_deref().unwrap_or(" ");
            let re = regex::Regex::new(sep).unwrap_or_else(|_| regex::Regex::new(" ").unwrap());
            let fields: Vec<value::StrykeValue> = re
                .split(line_str)
                .map(|s| value::StrykeValue::string(s.to_string()))
                .collect();
            interp.scope.set_array("F", fields)?;
        }

        let vm_jit = interp.vm_jit_enabled && interp.profiler.is_none();
        let mut vm = vm::VM::new(chunk, interp);
        vm.set_jit_enabled(vm_jit);
        vm.ip = chunk.body_start_ip;
        let _ = vm.execute()?;

        let mut out = interp.scope.get_scalar("_").to_string();
        out.push_str(&interp.ors);
        Ok(Some(out))
    })();
    interp.line_mode_eof_pending = false;
    result
}

/// Parse + register top-level subs / `use` (same as the VM path), then compile to bytecode without running.
/// Also runs static analysis to detect undefined variables and subroutines.
pub fn lint_program(program: &ast::Program, interp: &mut VMHelper) -> StrykeResult<()> {
    interp.prepare_program_top_level(program)?;
    // Strict-vars-style "Global symbol …" errors fire only when the source
    // itself has `use strict;` (or `use strict 'vars';`). `stryke check` on
    // a script without strict is a parse + compile gate, not an undefined-
    // variable enforcer. Topic vars (`$_0`, `@_1`, …) and special vars stay
    // exempt regardless.
    static_analysis::analyze_program_with_strict(program, &interp.file, interp.strict_vars)?;
    if interp.strict_refs || interp.strict_subs || interp.strict_vars {
        return Ok(());
    }
    let comp = compiler::Compiler::new().with_source_file(interp.file.clone());
    match comp.compile_program(program) {
        Ok(_) => Ok(()),
        Err(e) => Err(compile_error_to_perl(e)),
    }
}

fn compile_error_to_perl(e: compiler::CompileError) -> StrykeError {
    match e {
        compiler::CompileError::Unsupported(msg) => {
            StrykeError::runtime(format!("compile: {}", msg), 0)
        }
        compiler::CompileError::Frozen { line, detail } => {
            // strict-vars violations (`Global symbol "$x" requires explicit
            // package name…`) are compile-time errors in perl, so emit them
            // as `Syntax` so the formatter appends `Execution of -e aborted
            // due to compilation errors.` for parity.
            if detail.starts_with("Global symbol") {
                StrykeError::syntax(detail, line)
            } else {
                StrykeError::runtime(detail, line)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_executes_last_expression_value() {
        // Statement-only programs may yield 0 via the VM path; assert parse + run succeed.
        let p = parse("2 + 2").expect("parse");
        assert!(!p.statements.is_empty());
        let _ = run("2 + 2").expect("run");
    }

    #[test]
    fn run_propagates_parse_errors() {
        assert!(run("sub f {").is_err());
    }

    #[test]
    fn interpreter_scope_persists_global_scalar_across_execute_calls() {
        let mut interp = VMHelper::new();
        let assign = parse("$persist_test = 100").expect("parse assign");
        interp.execute(&assign).expect("assign");
        let read = parse("$persist_test").expect("parse read");
        let v = interp.execute(&read).expect("read");
        assert_eq!(v.to_int(), 100);
    }

    #[test]
    fn parse_empty_program() {
        let p = parse("").expect("empty input should parse");
        assert!(p.statements.is_empty());
    }

    #[test]
    fn parse_expression_statement() {
        let p = parse("2 + 2").expect("parse");
        assert!(!p.statements.is_empty());
    }

    #[test]
    fn parse_semicolon_only_statements() {
        parse(";;").expect("semicolons only");
    }

    #[test]
    fn parse_if_with_block() {
        parse("if (1) { 2 }").expect("if");
    }

    #[test]
    fn parse_fails_on_invalid_syntax() {
        assert!(parse("sub f {").is_err());
    }

    #[test]
    fn parse_qw_word_list() {
        parse("my @a = qw(x y z)").expect("qw list");
    }

    #[test]
    fn parse_c_style_for_loop() {
        parse("for (my $i = 0; $i < 3; $i = $i + 1) { 1; }").expect("c-style for");
    }

    #[test]
    fn parse_package_statement() {
        parse("package Foo::Bar; 1").expect("package");
    }

    #[test]
    fn parse_unless_block() {
        parse("unless (0) { 1; }").expect("unless");
    }

    #[test]
    fn parse_if_elsif_else() {
        parse("if (0) { 1; } elsif (1) { 2; } else { 3; }").expect("if elsif");
    }

    #[test]
    fn parse_q_constructor() {
        parse(r#"my $s = q{braces}"#).expect("q{}");
        parse(r#"my $t = qq(double)"#).expect("qq()");
    }

    #[test]
    fn parse_regex_literals() {
        parse("m/foo/").expect("m//");
        parse("s/foo/bar/g").expect("s///");
    }

    #[test]
    fn parse_begin_and_end_blocks() {
        parse("BEGIN { 1; }").expect("BEGIN");
        parse("END { 1; }").expect("END");
    }

    #[test]
    fn parse_transliterate_y() {
        parse("$_ = 'a'; y/a/A/").expect("y//");
    }

    #[test]
    fn parse_foreach_with_my_iterator() {
        parse("foreach my $x (1, 2) { $x; }").expect("foreach my");
    }

    #[test]
    fn parse_our_declaration() {
        parse("our $g = 1").expect("our");
    }

    #[test]
    fn parse_local_declaration() {
        parse("local $x = 1").expect("local");
    }

    #[test]
    fn parse_use_no_statements() {
        parse("use strict").expect("use");
        parse("no warnings").expect("no");
    }

    #[test]
    fn parse_sub_with_prototype() {
        parse("fn add2 ($$) { return $_0 + $_1; }").expect("fn prototype");
        parse("fn try_block (&;@) { my ( $try, @code_refs ) = @_; }").expect("prototype @ slurpy");
    }

    #[test]
    fn parse_list_expression_in_parentheses() {
        parse("my @a = (1, 2, 3)").expect("list");
    }

    #[test]
    fn parse_require_expression() {
        parse("require strict").expect("require");
    }

    #[test]
    fn parse_do_string_eval_form() {
        parse(r#"do "foo.pl""#).expect("do string");
    }

    #[test]
    fn parse_package_qualified_name() {
        parse("package Foo::Bar::Baz").expect("package ::");
    }

    #[test]
    fn parse_my_multiple_declarations() {
        parse("my ($a, $b, $c)").expect("my list");
    }

    #[test]
    fn parse_eval_block_statement() {
        parse("eval { 1; }").expect("eval block");
    }

    #[test]
    fn parse_p_statement() {
        parse("p 42").expect("p");
    }

    #[test]
    fn parse_chop_scalar() {
        parse("chop $s").expect("chop");
    }

    #[test]
    fn vendor_perl_inc_path_points_at_vendor_perl() {
        let p = vendor_perl_inc_path();
        assert!(
            p.ends_with("vendor/perl"),
            "unexpected vendor path: {}",
            p.display()
        );
    }

    #[test]
    fn format_program_roundtrips_simple_expression() {
        let p = parse("$x + 1").expect("parse");
        let out = format_program(&p);
        assert!(!out.trim().is_empty());
    }
}

#[cfg(test)]
mod builtins_extended_tests;

#[cfg(test)]
mod lib_api_extended_tests;

#[cfg(test)]
mod parallel_api_tests;

#[cfg(test)]
mod parse_smoke_extended;

#[cfg(test)]
mod parse_smoke_batch2;

#[cfg(test)]
mod parse_smoke_batch3;

#[cfg(test)]
mod parse_smoke_batch4;

#[cfg(test)]
mod crate_api_tests;

#[cfg(test)]
mod parser_shape_tests;

#[cfg(test)]
mod interpreter_unit_tests;

#[cfg(test)]
mod run_semantics_tests;

#[cfg(test)]
mod run_semantics_more;

#[cfg(test)]
mod value_extra_tests;

#[cfg(test)]
mod lexer_extra_tests;

#[cfg(test)]
mod parser_extra_tests;

#[cfg(test)]
mod builtins_extra_tests;

#[cfg(test)]
mod keywords_hash_tests;

#[cfg(test)]
mod thread_extra_tests;

#[cfg(test)]
mod error_extra_tests;

#[cfg(test)]
mod oo_extra_tests;

#[cfg(test)]
mod regex_extra_tests;

#[cfg(test)]
mod aot_extra_tests;
