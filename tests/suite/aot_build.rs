//! End-to-end test for `stryke build` AOT binaries: compile a Perl script to a standalone
//! executable, run the result in a separate process, and verify stdout + exit code.
//!
//! Does not require `rustc` (FFI is exercised in unit tests). Skips cleanly on Windows
//! where dlopen / POSIX permissions don't apply in v1.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "stryke-aot-e2e-{}-{}-{}",
        std::process::id(),
        tag,
        rand::random::<u32>()
    ))
}

#[test]
fn aot_build_and_run_hello_script() {
    let exe = env!("CARGO_BIN_EXE_st");
    let script = tmp_path("hello.pl");
    let bin = tmp_path("hello_bin");

    fs::write(
        &script,
        "my $who = $ARGV[0] // 'world';\nprint \"hi $who\\n\";\n",
    )
    .unwrap();

    // Build the binary via `stryke build`.
    let build = Command::new(exe)
        .arg("build")
        .arg(&script)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("spawn stryke build");
    assert!(
        build.status.success(),
        "stryke build failed: stderr={}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(bin.exists(), "built binary missing at {}", bin.display());
    // Binary must be executable.
    let meta = fs::metadata(&bin).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(meta.permissions().mode() & 0o111, 0, "not executable");

    // Run the built binary with a CLI argument — all args must reach `@ARGV`.
    let run = Command::new(&bin)
        .arg("alice")
        .output()
        .expect("spawn built binary");
    assert!(
        run.status.success(),
        "built binary failed: stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hi alice\n");

    fs::remove_file(&script).ok();
    fs::remove_file(&bin).ok();
}

#[test]
fn aot_build_preserves_exit_code_from_die() {
    let exe = env!("CARGO_BIN_EXE_st");
    let script = tmp_path("fail.pl");
    let bin = tmp_path("fail_bin");
    fs::write(&script, "die \"oops\\n\";\n").unwrap();

    let build = Command::new(exe)
        .arg("build")
        .arg(&script)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("spawn stryke build");
    assert!(
        build.status.success(),
        "stryke build failed: stderr={}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(&bin).output().expect("spawn built binary");
    assert!(
        !run.status.success(),
        "expected built binary to exit nonzero on die"
    );
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("oops"),
        "expected die message on stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    fs::remove_file(&script).ok();
    fs::remove_file(&bin).ok();
}

#[test]
fn aot_build_rejects_syntax_error_at_build_time() {
    let exe = env!("CARGO_BIN_EXE_st");
    let script = tmp_path("bad.pl");
    let bin = tmp_path("bad_bin");
    fs::write(&script, "sub oops {\n").unwrap();

    let build = Command::new(exe)
        .arg("build")
        .arg(&script)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("spawn stryke build");
    assert!(
        !build.status.success(),
        "stryke build should reject malformed source at build time"
    );
    assert!(
        !bin.exists() || fs::metadata(&bin).map(|m| m.len()).unwrap_or(0) < 1024,
        "malformed build should not produce a large output binary"
    );

    fs::remove_file(&script).ok();
    let _ = fs::remove_file(&bin);
}

#[test]
fn aot_build_help_subcommand_prints_usage() {
    let exe = env!("CARGO_BIN_EXE_st");
    let out = Command::new(exe)
        .arg("build")
        .arg("--help")
        .output()
        .expect("spawn stryke build --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("usage:"), "no usage line: {}", stdout);
    assert!(stdout.contains("build"), "no 'build' in help: {}", stdout);
}
