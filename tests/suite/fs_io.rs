//! Filesystem builtins with real temp paths (headless-safe, no network).

use crate::common::*;
use perlrs::interpreter::Interpreter;
use std::path::PathBuf;

#[test]
fn mkdir_creates_directory_and_file_test_sees_it() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("perlrs_itest_mkdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let p = dir.to_str().expect("temp path utf-8");
    let code = format!(r#"mkdir("{p}", 0755); (-e "{p}" ? 1 : 0)"#);
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unlink_returns_zero_for_missing_file() {
    assert_eq!(
        eval_int(r#"unlink("/nonexistent_path_perlrs_itest_01234")"#),
        0
    );
}

#[test]
fn rename_moves_file() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("perlrs_itest_rename_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a");
    let b = dir.join("b");
    std::fs::write(&a, "x").unwrap();
    let pa = a.to_str().expect("utf-8");
    let pb = b.to_str().expect("utf-8");
    let code = format!(r#"rename("{pa}", "{pb}"); (-e "{pb}" ? 1 : 0)"#);
    assert_eq!(eval_int(&code), 1);
    assert!(!a.exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// IO::File-style methods on handle values (`$fh->print`, `->getline`, `->close`).
#[test]
fn filehandle_method_io_print_getline_close() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("perlrs_itest_fhmethods_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.txt");
    let p = path.to_str().expect("utf-8");
    let code = format!(
        r#"
        use feature 'say';
        open(FH, '>', '{p}');
        FH->print("a");
        FH->say("b");
        FH->printf("%s", "c");
        FH->flush();
        FH->close();
        open(FH, '<', '{p}');
        my $line = FH->getline();
        FH->close();
        $line eq "ab\n" ? 1 : 0;
    "#
    );
    assert_eq!(eval_int(&code), 1);
    let body = std::fs::read_to_string(&path).expect("read out file");
    assert!(
        body.contains('a') && body.contains('b') && body.contains('c'),
        "body={body:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Piped `open` plus `<FH>` readline: bytecode (`execute`) and tree-walker (`execute_tree`) must agree.
#[cfg(unix)]
#[test]
fn piped_open_readline_vm_matches_tree_walker() {
    let code = r#"
        open(FH, "-|", "echo hi");
        my $x = <FH>;
        close FH;
        $x;
    "#;
    let program = perlrs::parse(code).expect("parse");
    let mut vm_interp = Interpreter::new();
    let v_vm = vm_interp.execute(&program).expect("execute vm");
    let mut tree_interp = Interpreter::new();
    let v_tree = tree_interp.execute_tree(&program).expect("execute tree");
    assert_eq!(v_vm.to_string(), v_tree.to_string());
    assert!(v_vm.to_string().contains("hi"));
}

#[cfg(unix)]
#[test]
fn chmod_sets_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir: PathBuf =
        std::env::temp_dir().join(format!("perlrs_itest_chmod_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("f");
    std::fs::write(&f, "x").unwrap();
    let pf = f.to_str().expect("utf-8");
    let code = format!(r#"chmod(0600, "{pf}")"#);
    assert_eq!(eval_int(&code), 1);
    let m = std::fs::metadata(&f).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o600);
    std::fs::remove_dir_all(&dir).ok();
}
