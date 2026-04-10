//! `pe --profile`: wall-clock samples on stderr (VM opcode lines + subs; flamegraph-ready).

use std::process::Command;

#[test]
fn pe_profile_stderr_has_vm_report_sections() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let out = Command::new(exe)
        .args(["--profile", "-e", "sub f { 1 } f();"])
        .output()
        .expect("spawn pe");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("perlrs --profile: collapsed stacks"),
        "expected flamegraph folded header, got: {stderr:?}"
    );
    assert!(
        stderr.contains("perlrs --profile: lines"),
        "expected line totals header, got: {stderr:?}"
    );
    assert!(
        stderr.contains("perlrs --profile: subs"),
        "expected subs header, got: {stderr:?}"
    );
}
