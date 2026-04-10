//! `-n` / `-p` over stdin: the driver must release the stdin lock between lines so `<>` / `readline`
//! inside the `-e` body can acquire it. Otherwise the body blocks forever (exclusive `StdinLock`).

use std::io::Write;
use std::process::{Command, Stdio};

fn perlrs_exe() -> &'static str {
    env!("CARGO_BIN_EXE_perlrs")
}

/// Body `<>` reads the next line after `$_` (Perl); must not deadlock with the outer line loop.
#[test]
fn line_mode_n_stdin_body_readline_prints_next_line() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"print <>"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"a\nb\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b\n");
}

/// After the last line, `<>` in the body sees EOF (undef) and must not block.
#[test]
fn line_mode_n_stdin_body_readline_after_eof_returns_undef_without_hang() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"print <>"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"a\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
}

/// `-l` + `-p`: chomped `$_` is printed with `$\` (default newline) after each line — multi-line must not concatenate.
#[test]
fn line_mode_lpe_implicit_print_appends_ors_each_line() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-lpe", r#"$_=uc"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"a\nb\nc\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "A\nB\nC\n");
}
