//! `fo --profile`: wall-clock samples on stderr (VM opcode lines + subs; flamegraph-ready).

use std::process::Command;

#[test]
fn pe_profile_stderr_has_vm_report_sections() {
    let exe = env!("CARGO_BIN_EXE_fo");
    let out = Command::new(exe)
        .args(["--profile", "-e", "sub foo { 1 } foo();"])
        .output()
        .expect("spawn fo");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("forge --profile: collapsed stacks"),
        "expected flamegraph folded header, got: {stderr:?}"
    );
    assert!(
        stderr.contains("forge --profile: lines"),
        "expected line totals header, got: {stderr:?}"
    );
    assert!(
        stderr.contains("forge --profile: subs"),
        "expected subs header, got: {stderr:?}"
    );
}
