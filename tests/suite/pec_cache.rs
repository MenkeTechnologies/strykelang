//! End-to-end tests for the `.pec` bytecode cache: `FORGE_BC_CACHE=1` should produce a
//! cache file on first run and successfully load it on the second.
//!
//! These run the real `fo` binary in a child process so the test exercises the same
//! main.rs wiring users hit. Cache directory is per-test under `$TMPDIR` to keep runs
//! isolated and avoid clobbering the developer's `~/.cache/forge/bc`.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forge-pec-{}-{}-{}",
        std::process::id(),
        tag,
        rand::random::<u32>()
    ))
}

fn run_with_cache(exe: &str, cache_dir: &PathBuf, script: &PathBuf) -> std::process::Output {
    Command::new(exe)
        .env("FORGE_BC_CACHE", "1")
        .env("FORGE_BC_DIR", cache_dir)
        .arg(script)
        .output()
        .expect("spawn fo")
}

#[test]
fn pec_first_run_writes_cache_warm_run_reuses_it() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let script = tmp_path("simple.pl");
    let cache_dir = tmp_path("cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(&script, "my $s = 0; $s += $_ for 1..10; print \"$s\\n\";\n").unwrap();

    // Cold: cache empty → first run must compile and persist a `.pec` file.
    let cold = run_with_cache(exe, &cache_dir, &script);
    assert!(
        cold.status.success(),
        "cold run failed: {}",
        String::from_utf8_lossy(&cold.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&cold.stdout), "55\n");

    let pec_files: Vec<_> = fs::read_dir(&cache_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".pec"))
        .collect();
    assert_eq!(
        pec_files.len(),
        1,
        "expected exactly one .pec file after cold run, got {}",
        pec_files.len()
    );
    let pec_size = fs::metadata(pec_files[0].path()).unwrap().len();
    assert!(pec_size > 0, ".pec file is empty");

    // Warm: same script → must produce identical output without rewriting the cache.
    let cache_mtime_before = fs::metadata(pec_files[0].path())
        .unwrap()
        .modified()
        .unwrap();
    let warm = run_with_cache(exe, &cache_dir, &script);
    assert!(
        warm.status.success(),
        "warm run failed: {}",
        String::from_utf8_lossy(&warm.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&warm.stdout), "55\n");
    let cache_mtime_after = fs::metadata(pec_files[0].path())
        .unwrap()
        .modified()
        .unwrap();
    // The save path is gated on `pec_cache_fingerprint` being set, which is taken on cache
    // miss only — so a warm hit must NOT rewrite the file.
    assert_eq!(
        cache_mtime_before, cache_mtime_after,
        "warm hit must not re-save the .pec file"
    );

    fs::remove_file(&script).ok();
    fs::remove_dir_all(&cache_dir).ok();
}

#[test]
fn pec_source_change_invalidates_cache() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let script = tmp_path("changing.pl");
    let cache_dir = tmp_path("cache_inval");
    fs::create_dir_all(&cache_dir).unwrap();

    // Run #1.
    fs::write(&script, "print \"v1\\n\";\n").unwrap();
    let r1 = run_with_cache(exe, &cache_dir, &script);
    assert_eq!(String::from_utf8_lossy(&r1.stdout), "v1\n");

    // Edit the script: a new fingerprint should be generated, producing a SECOND .pec file
    // alongside the first (we don't garbage-collect the old one in v1).
    fs::write(&script, "print \"v2\\n\";\n").unwrap();
    let r2 = run_with_cache(exe, &cache_dir, &script);
    assert_eq!(
        String::from_utf8_lossy(&r2.stdout),
        "v2\n",
        "edited script must produce new output, not stale cache hit"
    );

    let count = fs::read_dir(&cache_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.file_name().to_string_lossy().ends_with(".pec"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 2, "expected one .pec file per source version");

    fs::remove_file(&script).ok();
    fs::remove_dir_all(&cache_dir).ok();
}

#[test]
fn pec_disabled_by_default_no_cache_writes() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let script = tmp_path("nocache.pl");
    let cache_dir = tmp_path("cache_off");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(&script, "print \"hi\\n\";\n").unwrap();

    // No `FORGE_BC_CACHE=1` → cache must be inert.
    let out = Command::new(exe)
        .env("FORGE_BC_DIR", &cache_dir)
        .arg(&script)
        .output()
        .expect("spawn fo");
    assert!(out.status.success());
    let count = fs::read_dir(&cache_dir).unwrap().count();
    assert_eq!(
        count, 0,
        "cache directory must stay empty when FORGE_BC_CACHE is unset"
    );

    fs::remove_file(&script).ok();
    fs::remove_dir_all(&cache_dir).ok();
}

#[test]
fn pec_disabled_for_dash_e_oneliners() {
    // One-liners must NOT touch the cache: warm load is slower than parse+compile for
    // tiny scripts (measured ~2-3×), and unique `-e` invocations would pollute the cache
    // directory with no GC. The gate in main.rs is the contract this test pins.
    let exe = env!("CARGO_BIN_EXE_pe");
    let cache_dir = tmp_path("cache_oneliner");
    fs::create_dir_all(&cache_dir).unwrap();

    let out = Command::new(exe)
        .env("FORGE_BC_CACHE", "1")
        .env("FORGE_BC_DIR", &cache_dir)
        .arg("-e")
        .arg("print 7+8")
        .output()
        .expect("spawn fo -e");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "15");

    let pec_files = fs::read_dir(&cache_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .map(|e| e.file_name().to_string_lossy().ends_with(".pec"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        pec_files, 0,
        "`-e` invocation must not write a .pec file (gate in main.rs)"
    );

    fs::remove_dir_all(&cache_dir).ok();
}

#[test]
fn pec_warm_run_preserves_runtime_errors() {
    // Cache only stores the bytecode + program — runtime errors must still surface
    // identically on warm runs.
    let exe = env!("CARGO_BIN_EXE_pe");
    let script = tmp_path("die.pl");
    let cache_dir = tmp_path("cache_die");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(&script, "die \"boom\\n\";\n").unwrap();

    let cold = run_with_cache(exe, &cache_dir, &script);
    assert!(!cold.status.success(), "cold run should fail with die");
    assert!(
        String::from_utf8_lossy(&cold.stderr).contains("boom"),
        "missing die message on cold run"
    );

    let warm = run_with_cache(exe, &cache_dir, &script);
    assert!(!warm.status.success(), "warm run should fail with die");
    assert!(
        String::from_utf8_lossy(&warm.stderr).contains("boom"),
        "missing die message on warm run"
    );

    fs::remove_file(&script).ok();
    fs::remove_dir_all(&cache_dir).ok();
}
