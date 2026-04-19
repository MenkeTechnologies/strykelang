//! Program from stdin: a piped script must be read in full (not only the first line).
//! `perl -` / `forge -` reads the program from stdin even when a TTY would otherwise start the REPL.

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn forge_reads_full_multiline_script_from_stdin_pipe() {
    let exe = env!("CARGO_BIN_EXE_forge");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn forge");

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

#[test]
fn forge_dash_reads_program_from_stdin() {
    let exe = env!("CARGO_BIN_EXE_forge");
    let mut child = Command::new(exe)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn forge -");

    let script = "print \"ok\\n\";";
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(script.as_bytes()).unwrap();
    drop(stdin);

    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
}

#[test]
fn pe_reads_full_multiline_script_from_stdin_pipe() {
    let exe = env!("CARGO_BIN_EXE_fo");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn fo");

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
