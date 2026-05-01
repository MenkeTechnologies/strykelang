//! Zsh-style glob qualifiers — `(/)`, `(.)`, `(@)`, `(*)`, `(L+N)`, `(om[N])`, `(N)` —
//! delegated to the zshrs glob engine. World-first: zsh glob qualifiers in a
//! scripting language. `c("**(/)")` (slurp directories) hard-fails because
//! slurping a non-file is meaningless.

use crate::common::*;
use std::path::PathBuf;

fn fixture_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "stryke_glob_qual_{}_{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub/deeper")).unwrap();
    std::fs::write(dir.join("file1.txt"), "alpha\n").unwrap();
    std::fs::write(dir.join("file2.txt"), "bravo\n").unwrap();
    std::fs::write(dir.join("sub/file3.txt"), "charlie\n").unwrap();
    std::fs::write(dir.join("sub/deeper/file4.txt"), "delta\n").unwrap();
    dir
}

#[test]
fn slurp_dot_qualifier_concatenates_only_regular_files_recursively() {
    let dir = fixture_dir("dot");
    let p = dir.to_str().unwrap();
    let code = format!(r#"chdir("{p}"); slurp("**(.)")"#);
    let out = eval_string(&code);
    let mut parts: Vec<&str> = out.lines().collect();
    parts.sort();
    assert_eq!(parts, vec!["alpha", "bravo", "charlie", "delta"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn slurp_slash_qualifier_directories_hard_fails() {
    let dir = fixture_dir("slash");
    let p = dir.to_str().unwrap();
    let code = format!(r#"chdir("{p}"); slurp("**(/)")"#);
    let kind = eval_err_kind(&code);
    assert_eq!(kind, stryke::error::ErrorKind::Runtime);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn glob_slash_qualifier_lists_directories_recursively() {
    let dir = fixture_dir("globslash");
    let p = dir.to_str().unwrap();
    let code =
        format!(r#"chdir("{p}"); my @d = glob("**(/)"); join(",", sort @d)"#);
    let out = eval_string(&code);
    assert!(out.contains("./sub"), "got {out}");
    assert!(out.contains("./sub/deeper"), "got {out}");
    // No regular files leak into the directory listing.
    assert!(!out.contains("file1.txt"), "got {out}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn glob_dot_qualifier_lists_regular_files_recursively() {
    let dir = fixture_dir("globdot");
    let p = dir.to_str().unwrap();
    let code =
        format!(r#"chdir("{p}"); my @f = glob("**(.)"); join(",", sort @f)"#);
    let out = eval_string(&code);
    for f in ["file1.txt", "file2.txt", "file3.txt", "file4.txt"] {
        assert!(out.contains(f), "missing {f}: {out}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn glob_n_qualifier_returns_empty_on_no_match() {
    let dir = fixture_dir("globn");
    let p = dir.to_str().unwrap();
    let code = format!(
        r#"chdir("{p}"); my @f = glob("nope-this-cannot-match*(N)"); scalar(@f)"#
    );
    assert_eq!(eval_int(&code), 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn glob_size_qualifier_filters_by_byte_count() {
    let dir = fixture_dir("globsize");
    // file1.txt is 6 bytes ("alpha\n"); a 0-byte file shouldn't match (L+1).
    std::fs::write(dir.join("empty"), "").unwrap();
    let p = dir.to_str().unwrap();
    let code = format!(
        r#"chdir("{p}"); my @f = glob("**(L+1)"); my $hit = 0; for (@f) {{ $hit = 1 if /file1\.txt/ }} $hit"#
    );
    assert_eq!(eval_int(&code), 1);
    let code_neg = format!(
        r#"chdir("{p}"); my @f = glob("**(L+1)"); my $hit = 0; for (@f) {{ $hit = 1 if /empty/ }} $hit"#
    );
    assert_eq!(eval_int(&code_neg), 0);
    std::fs::remove_dir_all(&dir).ok();
}
