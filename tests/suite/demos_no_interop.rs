//! CI pin: every public demo under `examples/*.stk` must run clean
//! under `stryke --no-interop`.
//!
//! Rationale per CLAUDE.md: `--no-interop` is the bot firewall — it
//! enforces stryke idioms at parse time (no `scalar`, no `length`,
//! no `reverse`, no `$a`/`$b` magic). Demos are the public face of
//! the language; they must showcase idiomatic stryke, not Perl
//! transliterations.
//!
//! Tests are gated on the presence of a built binary (`target/debug/s`
//! preferred; falls back to `target/release/s`). If neither exists
//! the assertion is skipped — local-dev workflows that haven't run
//! `cargo build` yet aren't penalized.

use std::path::PathBuf;
use std::process::Command;

use crate::common::GLOBAL_FLAGS_LOCK;

fn stryke_binary() -> Option<PathBuf> {
    // Prefer the freshest of {target/debug/s, target/release/s}.
    // `s` is the short binary alias the demos invoke via shebang.
    let cands = [
        "target/release/s",
        "target/debug/s",
        "target/release/stryke",
        "target/debug/stryke",
    ];
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for cand in cands {
        let p = PathBuf::from(cand);
        if let Ok(meta) = std::fs::metadata(&p) {
            if let Ok(m) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| m > *t) {
                    best = Some((p, m));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

fn run_demo(path: &str) -> Result<(), String> {
    let bin = match stryke_binary() {
        Some(b) => b,
        None => return Ok(()), // skip: no built binary
    };
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let out = Command::new(&bin)
        .args(["--no-interop", path])
        .output()
        .map_err(|e| format!("spawn {bin:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "demo {path} failed under --no-interop (exit={:?}):\nstderr:\n{}\nstdout-tail:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .rev()
                .take(10)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(())
}

macro_rules! demo_runs_no_interop {
    ($fn_name:ident, $path:expr) => {
        #[test]
        fn $fn_name() {
            if let Err(e) = run_demo($path) {
                panic!("{}", e);
            }
        }
    };
}

demo_runs_no_interop!(demo_kvstore_basics,        "examples/kvstore_basics.stk");
demo_runs_no_interop!(demo_kvstore_cache,         "examples/kvstore_cache.stk");
demo_runs_no_interop!(demo_kvstore_namespace,     "examples/kvstore_namespace.stk");
demo_runs_no_interop!(demo_sketch_algebra,        "examples/sketch_algebra.stk");
demo_runs_no_interop!(demo_sketches_tier2,        "examples/sketches_tier2.stk");
demo_runs_no_interop!(demo_numerical_ids,         "examples/numerical_ids.stk");
demo_runs_no_interop!(demo_shell_repl,            "examples/shell_repl.stk");
demo_runs_no_interop!(demo_pipe_forward,          "examples/pipe_forward.stk");
demo_runs_no_interop!(demo_thread_macro,          "examples/thread_macro.stk");
demo_runs_no_interop!(demo_implicit_params,       "examples/implicit_params.stk");
demo_runs_no_interop!(demo_parallel_primitives,   "examples/parallel_primitives.stk");
demo_runs_no_interop!(demo_reflection_hashes,     "examples/reflection_hashes.stk");
demo_runs_no_interop!(demo_oop_classes,           "examples/oop_classes.stk");
demo_runs_no_interop!(demo_algebraic_match,       "examples/algebraic_match.stk");
demo_runs_no_interop!(demo_aop_intercepts,        "examples/aop_intercepts.stk");
demo_runs_no_interop!(demo_regex_three_tier,      "examples/regex_three_tier.stk");
demo_runs_no_interop!(demo_glob_qualifiers,       "examples/glob_qualifiers.stk");
demo_runs_no_interop!(demo_string_coordinates,    "examples/string_coordinates.stk");
demo_runs_no_interop!(demo_iterator_ops,          "examples/iterator_ops.stk");
demo_runs_no_interop!(demo_file_streams,          "examples/file_streams.stk");
demo_runs_no_interop!(demo_crypto,                "examples/crypto.stk");
demo_runs_no_interop!(demo_codecs,                "examples/codecs.stk");
demo_runs_no_interop!(demo_async_tasks,           "examples/async_tasks.stk");
demo_runs_no_interop!(demo_datetime,              "examples/datetime.stk");
demo_runs_no_interop!(demo_run_source,            "examples/run_source.stk");
