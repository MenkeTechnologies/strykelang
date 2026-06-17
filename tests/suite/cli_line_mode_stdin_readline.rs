//! `-n` / `-p` over stdin: the driver must release the stdin lock between lines so `<>` / `readline`
//! inside the `-e` body can acquire it. Otherwise the body blocks forever (exclusive `StdinLock`).

use std::io::Write;
use std::process::{Command, Stdio};

fn stryke_exe() -> &'static str {
    env!("CARGO_BIN_EXE_stryke")
}

/// Body `<>` reads the next line after `$_` (Perl); must not deadlock with the outer line loop.
#[test]
fn line_mode_n_stdin_body_readline_prints_next_line() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"print <>"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
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
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"print <>"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
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
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-lpe", r#"$_=uc"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
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

/// `-l` sets output record separator; implicit print after `-p` must still run for empty `$_`.
#[test]
fn line_mode_lpe_empty_lines_preserve_blank_records() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-lpe", r#""#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"a\n\nb\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\n\nb\n");
}

/// Regression: chomped line must not be rejoined; each record gets its own trailing `$\`.
#[test]
fn line_mode_lpe_multibyte_utf8_line_round_trips() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-lpe", r#"$_ = "«$_»""#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all("café\nrésumé\n".as_bytes())
        .expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "«café»\n«résumé»\n");
}

/// `die` / `warn` append `, <> line N.` after an implicit `-n` read (matches Perl 5).
#[test]
fn line_mode_die_includes_diamond_input_line_in_message() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-lane", r#"die if /pro/"#])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(b"a\nb\nc\nd\ne\nprofile\n")
        .expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(255));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(", <> line 6."),
        "expected Perl-style input line in die message, stderr={stderr:?}"
    );
}

/// `die` before any input read omits the `, <> line N.` clause (matches Perl 5).
#[test]
fn die_without_read_has_no_input_line_clause() {
    let exe = stryke_exe();
    let out = Command::new(exe)
        .args(["-e", "die"])
        .stderr(Stdio::piped())
        .output()
        .expect("run stryke");
    assert_eq!(out.status.code(), Some(255));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("Died at -e line 1."),
        "stderr={stderr:?}"
    );
    assert!(
        !stderr.contains(", <> line"),
        "unexpected input line clause, stderr={stderr:?}"
    );
}

/// Diamond `while (<>)` on stdin uses `<>`, not `<STDIN>`, in the die suffix (matches Perl 5).
#[test]
fn die_while_diamond_stdin_uses_angle_brackets_not_stdin() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"while (<>) { die }"#])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"hi\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(255));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(", <> line 1."),
        "expected diamond bracket, stderr={stderr:?}"
    );
    assert!(
        !stderr.contains("<STDIN>"),
        "did not expect explicit STDIN in message, stderr={stderr:?}"
    );
}

/// Explicit read from `STDIN` is reflected as `<STDIN>` in the die suffix (matches Perl 5).
#[test]
fn die_explicit_stdin_read_shows_stdin_in_message() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"$_ = <STDIN>; die"#])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"hi\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(255));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(", <STDIN> line 1."), "stderr={stderr:?}");
}

/// `END {}` runs **once** after the whole `-n` loop, not once per input line. Regression for
/// the compiled END region being re-executed on every line (END aggregation fired per-line).
#[test]
fn line_mode_n_end_block_fires_once_not_per_line() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"END { print "END\n" }"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"a\nb\nc\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "END\n");
}

/// The canonical aggregation idiom: accumulate per line in main, emit the total once in `END`.
/// `1+2+3+4 = 10`, printed a single time after the loop.
#[test]
fn line_mode_n_end_aggregation_emits_total_once() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"$s += $_; END { print "$s\n" }"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"1\n2\n3\n4\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n");
}

/// Regression: a `do { } if` block inside a loop in the per-line body must not crash with
/// "BlockReturnValue with empty call stack". The do-block's bytecode is relocated after the
/// main `Halt`; an earlier IP-arithmetic split of the line-mode chunk mis-pointed the post-loop
/// `END` runner into that relocated block body. (No `END` block here, so nothing runs after the
/// loop.) Two input lines × `for (1..2)` ⇒ four `F:` lines, no trailing garbage.
#[test]
fn line_mode_n_do_block_in_loop_does_not_crash_post_loop() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-lne", r#"for (1..2){ do{print "F:$_"} if m{(\S+)} }"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"x y\nz w\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "F:1\nF:2\nF:1\nF:2\n");
}

/// Multiple `END {}` blocks run once each in reverse (LIFO) declaration order under `-n`,
/// same as Perl: last declared runs first.
#[test]
fn line_mode_n_multiple_end_blocks_run_lifo_once_each() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"END { print "E1\n" } END { print "E2\n" }"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"a\nb\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "E2\nE1\n");
}

/// Non-line `-e`: multiple `END {}` blocks also run reverse (LIFO), once each — same compiler
/// path as line mode, so the ordering must match Perl here too.
#[test]
fn plain_e_multiple_end_blocks_run_lifo() {
    let exe = stryke_exe();
    let out = Command::new(exe)
        .args(["-e", r#"END { print "1\n" } END { print "2\n" } END { print "3\n" }"#])
        .stdout(Stdio::piped())
        .output()
        .expect("run stryke");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n2\n1\n");
}

/// `warn` uses the same input-line suffix as `die` under `-n` (matches Perl 5).
#[test]
fn warn_line_mode_includes_diamond_input_line_in_message() {
    let exe = stryke_exe();
    let mut child = Command::new(exe)
        .args(["-ne", r#"warn if /hi/"#])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn stryke");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin.write_all(b"hi\n").expect("write stdin");
    drop(stdin);
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Warning: something's wrong at -e line 1, <> line 1."),
        "stderr={stderr:?}"
    );
}
