//! `<>` / `<STDIN>` / `<FH>` in **list** context must read all lines until EOF (Perl `readline` list semantics).
//! Scalar context (`$x = <>`) stays one line. `reverse <>` passes list context into the diamond (zpwr patterns).

use perlrs::interpreter::Interpreter;
use std::io::Write;
use std::process::{Command, Stdio};

fn perlrs_exe() -> &'static str {
    env!("CARGO_BIN_EXE_perlrs")
}

#[test]
fn diamond_list_context_slurps_piped_stdin() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"my @a = <>; print scalar(@a)"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
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
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3");
}

#[test]
fn stdin_angle_bracket_list_context_slurps_piped_stdin() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"my @a = <STDIN>; print scalar(@a)"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
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
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3");
}

#[test]
fn diamond_list_context_join_concatenates_full_input() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"print join('', <>)"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"alpha\nbeta\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "alpha\nbeta\n");
}

#[test]
fn reverse_diamond_list_context_slurps_then_reverses_line_order() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"my @a = reverse <>; print $a[0]"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
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
    assert_eq!(String::from_utf8_lossy(&out.stdout), "c\n");
}

#[test]
fn empty_stdin_diamond_list_context_yields_zero_lines() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"my @a = <>; print scalar(@a)"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0");
}

#[test]
fn scalar_diamond_still_reads_only_first_line() {
    let exe = perlrs_exe();
    let mut child = Command::new(exe)
        .args(["-e", r#"my $x = <>; print $x"#])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn perlrs");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"first\nsecond\nthird\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "first\n");
}

/// Bytecode VM and tree-walker must agree on `<FH>` list slurp from a real file (not only stdin pipe).
#[test]
fn open_file_readline_list_vm_matches_tree_walker() {
    let path = std::env::temp_dir().join(format!(
        "perlrs_readline_list_{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, b"one\ntwo\nthree\n").expect("write temp");
    let ps = path.to_str().expect("utf-8 path");
    let code = format!(
        r#"
        open F, '<', '{ps}';
        my @a = <F>;
        close F;
        0+@a;
    "#
    );
    let program = perlrs::parse(&code).expect("parse");
    let mut vm_interp = Interpreter::new();
    let v_vm = vm_interp.execute(&program).expect("execute vm");
    let mut tree_interp = Interpreter::new();
    let v_tree = tree_interp.execute_tree(&program).expect("execute tree");
    assert_eq!(v_vm.to_int(), v_tree.to_int(), "vm={v_vm:?} tree={v_tree:?}");
    assert_eq!(v_vm.to_int(), 3);
    std::fs::remove_file(&path).ok();
}

/// `reverse <FH>`: list context through `reverse` (same zpwr pattern as `reverse <>` on stdin).
#[test]
fn open_file_reverse_readline_list_vm_matches_tree_walker() {
    let path = std::env::temp_dir().join(format!(
        "perlrs_reverse_slurp_{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, b"aa\nbb\ncc\n").expect("write temp");
    let ps = path.to_str().expect("utf-8 path");
    let code = format!(
        r#"
        open F, '<', '{ps}';
        my @a = reverse <F>;
        close F;
        $a[0];
    "#
    );
    let program = perlrs::parse(&code).expect("parse");
    let mut vm_interp = Interpreter::new();
    let v_vm = vm_interp.execute(&program).expect("execute vm");
    let mut tree_interp = Interpreter::new();
    let v_tree = tree_interp.execute_tree(&program).expect("execute tree");
    assert_eq!(v_vm.to_string(), v_tree.to_string());
    assert_eq!(v_vm.to_string(), "cc\n");
    std::fs::remove_file(&path).ok();
}
