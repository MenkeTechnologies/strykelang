//! Program from stdin: a piped script must be read in full (not only the first line).

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn pe_reads_full_multiline_script_from_stdin_pipe() {
    let exe = env!("CARGO_BIN_EXE_pe");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pe");

    let script = "#!/usr/bin/env perl\nsay 1;\nsay 2;\n";
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(script.as_bytes()).unwrap();
    drop(stdin);

    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n2\n");
}
