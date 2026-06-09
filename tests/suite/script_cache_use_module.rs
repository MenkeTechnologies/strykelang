//! Regression: rkyv bytecode cache must not corrupt the outer script's
//! entry when the script's top-level `use Module` triggers nested module
//! compilation.
//!
//! Bug repro (pre-fix): `prepare_program_top_level` in `try_vm_execute`
//! processes `use Module` BEFORE the outer's `cache_script_path` /
//! `cached_chunk` are claimed into locals. The `use` resolves via
//! `require_execute` → `parse_and_run_module_in_file` → recursive
//! `try_vm_execute(module_program)`. The nested call's
//! `interp.cache_script_path.take()` consumes the outer's path and
//! `script_cache::try_save(outer_path, module_program, module_chunk)`
//! writes the MODULE'S program+chunk under the OUTER's cache key. Every
//! cache hit afterward runs the module's body (sub-decls only → no output,
//! exit 0), and the script body never executes.
//!
//! User-visible symptom (reported 2026-06-09): `s activity_maintainer.stk`
//! prints once on the first invocation, then silently no-ops on every
//! subsequent run until the source file is touched. `use GUI` is the
//! trigger — `use strict` / `use warnings` short-circuit in `exec_use_stmt`
//! and never reach `require_execute`, so they don't corrupt the cache.
//!
//! Test method: spawn the `stryke` binary twice against the same source
//! file, with `HOME` pointing at a fresh temp dir so the cache lives at
//! `$tempdir/.stryke/scripts.rkyv` and doesn't pollute the user's real
//! shard. Both runs must produce identical output ending in the script
//! body's marker line — proves the cached chunk is the OUTER's, not the
//! module's.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn bin() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_BIN_EXE_stryke").expect("CARGO_BIN_EXE_stryke"))
}

fn fixture_inc() -> String {
    format!("{}/tests/fixtures/inc", env!("CARGO_MANIFEST_DIR"))
}

/// Run `stryke <script>` with `HOME=<home>` and `-I <fixture_inc>` so
/// `use Trivial` resolves to the in-tree fixture. Returns (stdout, exit).
fn run_with_home(home: &std::path::Path, script: &std::path::Path) -> (String, i32) {
    let mut cmd = Command::new(bin());
    cmd.env("HOME", home)
        .env_remove("STRYKE_CACHE")
        .arg("-I")
        .arg(fixture_inc())
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn stryke");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child.wait_with_output().expect("wait_with_output");
                return (
                    String::from_utf8_lossy(&output.stdout).into_owned(),
                    output.status.code().unwrap_or(-1),
                );
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    panic!("stryke timed out");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => panic!("try_wait: {e}"),
        }
    }
}

#[test]
fn use_module_does_not_corrupt_outer_script_cache_entry() {
    let home = tempfile::tempdir().expect("tempdir for HOME");
    let work = tempfile::tempdir().expect("tempdir for script");

    let script = work.path().join("outer.stk");
    // Outer script: `use Trivial` (triggers nested try_vm_execute on the
    // module's body) + a unique marker line that ONLY appears in the
    // outer's bytecode. If the cache entry under `outer.stk` is
    // accidentally the module's program+chunk, run 2 prints nothing.
    std::fs::write(
        &script,
        "use Trivial\n\
         p \"OUTER-MARKER \" . trivial_answer()\n",
    )
    .expect("write outer.stk");

    let (out1, rc1) = run_with_home(home.path(), &script);
    assert_eq!(rc1, 0, "run 1 exit code (stdout: {out1:?})");
    assert!(
        out1.contains("OUTER-MARKER 42"),
        "run 1 (cache miss) must print the outer's marker — got {out1:?}"
    );

    // Run 2: with the rkyv cache present, the outer's compiled chunk
    // should be loaded from cache and executed. Pre-fix this prints
    // NOTHING because the cache holds Trivial's program+chunk.
    let (out2, rc2) = run_with_home(home.path(), &script);
    assert_eq!(rc2, 0, "run 2 exit code (stdout: {out2:?})");
    assert!(
        out2.contains("OUTER-MARKER 42"),
        "run 2 (cache hit) must print the outer's marker — got {out2:?}. \
         If empty: the cache entry under {} contains Trivial's program+chunk \
         instead of outer.stk's. Fix: try_vm_execute must claim \
         cache_script_path / cached_chunk into locals BEFORE \
         prepare_program_top_level runs, so nested calls see None.",
        script.display()
    );

    // Run 3: belt-and-braces — the cache hit path must be repeatable.
    let (out3, rc3) = run_with_home(home.path(), &script);
    assert_eq!(rc3, 0, "run 3 exit code (stdout: {out3:?})");
    assert!(
        out3.contains("OUTER-MARKER 42"),
        "run 3 (cache hit) must still print the outer's marker — got {out3:?}"
    );
}
