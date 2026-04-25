//! Filesystem builtins with real temp paths (headless-safe, no network).

use crate::common::*;
use std::path::PathBuf;
use stryke::interpreter::Interpreter;

#[test]
fn mkdir_creates_directory_and_file_test_sees_it() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_mkdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let p = dir.to_str().expect("temp path utf-8");
    let code = format!(r#"mkdir("{p}", 0755); (-e "{p}" ? 1 : 0)"#);
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unlink_returns_zero_for_missing_file() {
    assert_eq!(
        eval_int(r#"unlink("/nonexistent_path_stryke_itest_01234")"#),
        0
    );
}

#[test]
fn rename_moves_file() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_rename_{}", std::process::id()));
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
        std::env::temp_dir().join(format!("stryke_itest_fhmethods_{}", std::process::id()));
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

/// `print` / `printf` with no LIST use `$_` (Perl 5), including on IO handle methods.
#[test]
fn print_and_printf_no_args_use_topic() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_print_topic_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.txt");
    let p = path.to_str().expect("utf-8");
    let code = format!(
        r#"
        open(FH, '>', '{p}');
        for (1, 2, 3) {{ FH->print(); }}
        $_ = 'x';
        FH->printf();
        FH->close();
        1;
    "#
    );
    assert_eq!(eval_int(&code), 1);
    let body = std::fs::read_to_string(&path).expect("read out file");
    assert_eq!(body, "123x", "body={body:?}");
    std::fs::remove_dir_all(&dir).ok();
}

/// Piped `open` plus `<FH>` readline validates piped open readline.
#[cfg(unix)]
#[test]
fn piped_open_readline_works() {
    let code = r#"
        open(FH, "-|", "echo hi");
        my $x = <FH>;
        close FH;
        $x;
    "#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = Interpreter::new();
    let v = interp.execute(&program).expect("execute");
    assert!(v.to_string().contains("hi"));
}

#[cfg(unix)]
#[test]
fn chmod_sets_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_chmod_{}", std::process::id()));
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

#[test]
fn rmdir_removes_empty_directory() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_rmdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.to_str().expect("utf-8");
    let code = format!(r#"rmdir("{p}")"#);
    assert_eq!(eval_int(&code), 1);
    assert!(!dir.exists());
}

#[test]
fn getcwd_contains_chdir_target() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_getcwd_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.to_str().expect("utf-8");
    let code = format!(
        r#"chdir("{p}"); index(getcwd(), "{p}") >= 0 && index(Cwd::getcwd(), "{p}") >= 0 ? 1 : 0"#
    );
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn utime_sets_times() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_utime_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("f");
    std::fs::write(&f, "x").unwrap();
    let pf = f.to_str().expect("utf-8");
    let code = format!(r#"utime(1_000_000, 2_000_000, "{pf}")"#);
    assert_eq!(eval_int(&code), 1);
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(&f).unwrap();
    assert_eq!(m.atime(), 1_000_000);
    assert_eq!(m.mtime(), 2_000_000);
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn umask_read_roundtrip() {
    let code = r#"
        my $u = umask();
        my $old = umask(022);
        umask($old);
        ($u == $old) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[cfg(unix)]
#[test]
fn pipe_builtin_rw_roundtrip() {
    let code = r#"
        pipe(RD, WR);
        print WR "ping\n";
        WR->flush();
        close WR;
        my $x = <RD>;
        close RD;
        $x eq "ping\n" ? 1 : 0;
    "#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = Interpreter::new();
    let v = interp.execute(&program).expect("execute");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn realpath_resolves_existing_file() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_realpath_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("f.txt");
    std::fs::write(&f, "x").unwrap();
    let pf = f.to_str().expect("utf-8");
    let code = format!(r#"my $r = realpath("{pf}"); ($r ne "" && -e $r) ? 1 : 0"#);
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn realpath_missing_returns_undef() {
    let p = format!("/nonexistent_stryke_realpath_{}", std::process::id());
    let code = format!(r#"defined(realpath("{p}")) ? 1 : 0"#);
    assert_eq!(eval_int(&code), 0);
}

#[test]
fn canonpath_collapses_dot_dot() {
    assert_eq!(eval_string(r#"canonpath("foo/../bar//baz")"#), "bar/baz");
}

#[test]
fn spurt_writes_bytes_round_trip_with_slurp() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_spurt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("out.txt");
    let pf = f.to_str().expect("utf-8");
    let code = format!(r#"spurt("{pf}", "hello\n"); (slurp("{pf}") eq "hello\n") ? 1 : 0"#);
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn spurt_write_file_alias_mkdir_option() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_spurt_mkdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let nested = dir.join("a/b/c.txt");
    let pn = nested.to_str().expect("utf-8");
    let code = format!(r#"write_file("{pn}", "z", {{ mkdir => 1 }}); (-e "{pn}" ? 1 : 0)"#);
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn copy_file_creates_destination() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_copy_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("src.txt");
    let b = dir.join("dst.txt");
    std::fs::write(&a, "payload").unwrap();
    let pa = a.to_str().expect("utf-8");
    let pb = b.to_str().expect("utf-8");
    let code = format!(r#"copy("{pa}", "{pb}") && (slurp("{pb}") eq "payload") ? 1 : 0"#);
    assert_eq!(eval_int(&code), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn copy_preserve_updates_mtime() {
    use std::os::unix::fs::MetadataExt;
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_copy_preserve_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("src.txt");
    let b = dir.join("dst.txt");
    std::fs::write(&a, "x").unwrap();
    let pa = a.to_str().expect("utf-8");
    let pb = b.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(r#"utime(1_234_000, 5_555_000, "{pa}")"#)),
        1
    );
    let code = format!(r#"copy("{pa}", "{pb}", {{ preserve => 1 }})"#);
    assert_eq!(eval_int(&code), 1);
    let m = std::fs::metadata(&b).unwrap();
    assert_eq!(m.atime(), 1_234_000);
    assert_eq!(m.mtime(), 5_555_000);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn basename_dirname_fileparse() {
    assert_eq!(eval_string(r#"basename("/foo/bar/baz.txt")"#), "baz.txt");
    assert_eq!(eval_string(r#"dirname("/foo/bar/baz.txt")"#), "/foo/bar");
    assert_eq!(
        eval_string(r#"scalar fileparse("/foo/bar/baz.pl", ".pl")"#),
        "baz"
    );
    assert_eq!(
        eval_string(r#"my @f = fileparse("/foo/bar/baz.pl", ".pl"); join "|", @f"#),
        "baz|/foo/bar|.pl"
    );
}

#[test]
fn gethostname_returns_non_empty() {
    let h = eval_string(r#"gethostname()"#);
    assert!(!h.is_empty());
}

#[cfg(unix)]
#[test]
fn uname_sysname_nonempty() {
    let s = eval_string(r#"uname()->{"sysname"}"#);
    assert!(!s.is_empty());
}

#[test]
fn read_bytes_preserves_raw_octets() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_rbytes_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("b.bin");
    std::fs::write(&f, [0xffu8, 0, 9, 10]).unwrap();
    let pf = f.to_str().expect("utf-8");
    let code = format!(r#"length(slurp_raw("{pf}"))"#);
    assert_eq!(eval_int(&code), 4);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn move_renames_file_like_rename() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_itest_move_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("a");
    let b = dir.join("b");
    std::fs::write(&a, "m").unwrap();
    let pa = a.to_str().expect("utf-8");
    let pb = b.to_str().expect("utf-8");
    let code = format!(r#"move("{pa}", "{pb}") && (slurp("{pb}") eq "m") ? 1 : 0"#);
    assert_eq!(eval_int(&code), 1);
    assert!(!a.exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[test]
fn which_finds_sh_on_path() {
    let p = eval_string(r#"which("sh")"#);
    assert!(!p.is_empty());
    assert!(p.contains("sh"), "path={p:?}");
}
