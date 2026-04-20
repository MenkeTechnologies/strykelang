//! README [0x02] Usage / stdin / `-j` examples: `stryke` must accept the documented invocations.
//! See `README.md` sections **USAGE**, **Stdin / `-n` / `-p` / `-i`**, and parallel primitives example.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn stryke() -> &'static str {
    env!("CARGO_BIN_EXE_st")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fib() -> PathBuf {
    repo_root().join("examples/fibonacci.pl")
}

fn assert_success(label: &str, out: &std::process::Output) {
    assert!(
        out.status.success(),
        "{label}: status={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn readme_stryke_dash_e_print_hello() {
    let out = Command::new(stryke())
        .args(["-e", r#"print "Hello, world!\n""#])
        .output()
        .expect("spawn");
    assert_success("stryke -e hello", &out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Hello, world!\n");
}

#[test]
fn readme_stryke_script_plus_argv() {
    let out = Command::new(stryke())
        .arg(fib())
        .args(["arg1", "arg2"])
        .output()
        .expect("spawn");
    assert_success("stryke script.pl args", &out);
}

#[test]
fn readme_stryke_lane_autosplit_field0() {
    let mut child = Command::new(stryke())
        .args(["-lane", "print $F[0]"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(b"a b\n").unwrap();
    let out = child.wait_with_output().expect("wait");
    assert_success("stryke -lane", &out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\n");
}

#[test]
fn readme_stryke_syntax_check() {
    let out = Command::new(stryke())
        .arg("-c")
        .arg(fib())
        .output()
        .expect("spawn");
    assert_success("stryke -c", &out);
    let msg = String::from_utf8_lossy(&out.stderr);
    assert!(msg.contains("syntax OK"), "stderr={msg:?}");
}

#[test]
fn readme_stryke_lint() {
    let out = Command::new(stryke())
        .arg("--lint")
        .arg(fib())
        .output()
        .expect("spawn");
    assert_success("stryke --lint", &out);
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(
        s.contains("compile OK") || s.contains("bytecode compile check skipped"),
        "stderr={s:?}"
    );
}

#[test]
fn readme_stryke_disasm() {
    let out = Command::new(stryke())
        .arg("--disasm")
        .arg(fib())
        .output()
        .expect("spawn");
    assert_success("stryke --disasm", &out);
    let s = String::from_utf8_lossy(&out.stderr);
    assert!(
        s.contains("name[") || s.contains("sub_entries"),
        "stderr={s:?}"
    );
}

#[test]
fn readme_stryke_ast_json() {
    let out = Command::new(stryke())
        .arg("--ast")
        .arg(fib())
        .output()
        .expect("spawn");
    assert_success("stryke --ast", &out);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.trim_start().starts_with('{') && s.contains("statements"),
        "stdout head={s:?}"
    );
}

#[test]
fn readme_stryke_fmt() {
    let out = Command::new(stryke())
        .arg("--fmt")
        .arg(fib())
        .output()
        .expect("spawn");
    assert_success("stryke --fmt", &out);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("use strict") || s.contains("fib"),
        "stdout={s:?}"
    );
}

#[test]
fn readme_stryke_profile() {
    let out = Command::new(stryke())
        .arg("--profile")
        .arg(fib())
        .output()
        .expect("spawn");
    assert_success("stryke --profile", &out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("profile") || stderr.contains("fib"),
        "stderr={stderr:?}"
    );
}

#[test]
fn readme_stryke_explain_e0001() {
    let out = Command::new(stryke())
        .args(["--explain", "E0001"])
        .output()
        .expect("spawn");
    assert_success("stryke --explain E0001", &out);
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("subroutine") || s.contains("sub"),
        "expected expanded hint, got {s:?}"
    );
}

#[test]
fn readme_stryke_ne_uc_topic() {
    let mut child = Command::new(stryke())
        .args(["-ne", "print uc $_"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(b"data\n").unwrap();
    let out = child.wait_with_output().expect("wait");
    assert_success("stryke -ne uc", &out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "DATA\n");
}

#[test]
fn readme_stryke_subst_pipe() {
    // README: `cat f.txt | stryke -pe 's/foo/bar/g'` — transform lines from stdin.
    let mut child = Command::new(stryke())
        .args(["-pe", "s/foo/bar/g"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"foo line\n")
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    assert_success("stryke -pe pipe", &out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bar line\n");
}

#[test]
fn readme_stryke_i_two_files() {
    let dir = std::env::temp_dir().join(format!("readme_stryke_i2_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let a = dir.join("file1");
    let b = dir.join("file2");
    fs::write(&a, "foo\n").unwrap();
    fs::write(&b, "foo\n").unwrap();

    let out = Command::new(stryke())
        .current_dir(&dir)
        .args(["-i", "-pe", "s/foo/bar/g"])
        .arg(&a)
        .arg(&b)
        .output()
        .expect("spawn");
    assert_success("stryke -i two files", &out);
    assert_eq!(fs::read_to_string(&a).unwrap(), "bar\n");
    assert_eq!(fs::read_to_string(&b).unwrap(), "bar\n");
}

#[test]
fn readme_stryke_i_bak_glob_txt() {
    let dir = std::env::temp_dir().join(format!("readme_stryke_ibak_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let t = dir.join("z.txt");
    fs::write(&t, "x\n").unwrap();

    let out = Command::new(stryke())
        .current_dir(&dir)
        .args(["-i.bak", "-pe", "s/x/y/g"])
        .arg("z.txt")
        .output()
        .expect("spawn");
    assert_success("stryke -i.bak", &out);
    assert_eq!(fs::read_to_string(&t).unwrap(), "y\n");
    let bak = dir.join("z.txt.bak");
    assert!(bak.is_file());
    assert_eq!(fs::read_to_string(&bak).unwrap(), "x\n");
}

#[test]
fn readme_stryke_a_f_autosplit() {
    let mut child = Command::new(stryke())
        .args(["-aF:", "-ne", "print $F[1]"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child.stdin.take().unwrap().write_all(b"a:b:c\n").unwrap();
    let out = child.wait_with_output().expect("wait");
    assert_success("stryke -aF:", &out);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), "b");
}

#[test]
fn readme_stryke_j_pmap() {
    let out = Command::new(stryke())
        .args([
            "-j",
            "2",
            "-e",
            "my @data = (1); pmap { $_ * 2 } @data; print qq{ok\\n};",
        ])
        .output()
        .expect("spawn");
    assert_success("stryke -j pmap", &out);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ok\n");
}

#[test]
fn readme_stryke_examples_scripts() {
    for rel in [
        "examples/fibonacci.stk",
        "examples/text_processing.stk",
        "examples/parallel_demo.stk",
    ] {
        let path = repo_root().join(rel);
        let out = Command::new(stryke()).arg(&path).output().expect("spawn");
        assert_success(rel, &out);
    }
}
