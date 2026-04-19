//! `-n` / `-p`: `eof` with no arguments is true on the last line of stdin or each `@ARGV` file (Perl parity).

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn ne_eof_no_args_true_on_last_stdin_line() {
    let exe = env!("CARGO_BIN_EXE_stryke");
    let mut child = Command::new(exe)
        .args(["-ne", r#"print "eof:", eof ? "Y" : "N", "\n""#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"a\nb\nc\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "eof:N\neof:N\neof:Y\n"
    );
}

/// `CORE::eof()` is a qualified call (not the dedicated `eof` AST); parity with bare `eof` in `-n`.
#[test]
fn ne_core_eof_no_args_true_on_last_stdin_line() {
    let exe = env!("CARGO_BIN_EXE_stryke");
    let mut child = Command::new(exe)
        .args(["-ne", r#"print "eof:", CORE::eof() ? "Y" : "N", "\n""#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"a\nb\nc\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "eof:N\neof:N\neof:Y\n"
    );
}

#[test]
fn ne_eof_no_args_per_argv_file_last_line() {
    let dir = std::env::temp_dir().join(format!("stryke_eof_argv_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    std::fs::write(&a, "x\n").unwrap();
    std::fs::write(&b, "y\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_stryke");
    let out = Command::new(exe)
        .current_dir(&dir)
        .args(["-ne", r#"print $ARGV, " eof:", eof ? "Y" : "N", "\n""#])
        .args([a.file_name().unwrap(), b.file_name().unwrap()])
        .output()
        .expect("spawn stryke");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a.txt eof:Y\nb.txt eof:Y\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Regex flip-flop with `eof` as the right bound (`perlop`); uses the same `eof` semantics as bare `eof`.
#[test]
fn ne_regex_flipflop_two_dot_eof_prints_from_match_through_eof() {
    let exe = env!("CARGO_BIN_EXE_stryke");
    let mut child = Command::new(exe)
        .args(["-ne", r#"print if m{^export}..eof"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"x\nexport FOO=1\nmid\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "export FOO=1\nmid\n");
}

#[test]
fn ne_regex_flipflop_three_dot_eof_exclusive_right_bound() {
    let exe = env!("CARGO_BIN_EXE_stryke");
    let mut child = Command::new(exe)
        .args(["-ne", r#"print if m{^export}...eof"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"x\nexport FOO=1\nmid\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "export FOO=1\nmid\n");
}
