//! Zsh-style glob qualifiers — `(/)`, `(.)`, `(@)`, `(*)`, `(L+N)`, `(om[N])`, `(N)` —
//! delegated to the zshrs glob engine. World-first: zsh glob qualifiers in a
//! scripting language. `c("**(/)")` (slurp directories) hard-fails because
//! slurping a non-file is meaningless.

use crate::common::*;
use std::path::PathBuf;

fn fixture_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("stryke_glob_qual_{}_{}", label, std::process::id()));
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
    with_global_flags(|| {
        let dir = fixture_dir("dot");
        let p = dir.to_str().unwrap();
        let code = format!(r#"chdir("{p}"); slurp("**(.)")"#);
        let out = eval_locked(&code).to_string();
        let mut parts: Vec<&str> = out.lines().collect();
        parts.sort();
        assert_eq!(parts, vec!["alpha", "bravo", "charlie", "delta"]);
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn slurp_slash_qualifier_directories_hard_fails() {
    with_global_flags(|| {
        let dir = fixture_dir("slash");
        let p = dir.to_str().unwrap();
        let code = format!(r#"chdir("{p}"); slurp("**(/)")"#);
        let kind = eval_err_kind_locked(&code);
        assert_eq!(kind, stryke::error::ErrorKind::Runtime);
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn glob_slash_qualifier_lists_directories_recursively() {
    with_global_flags(|| {
        let dir = fixture_dir("globslash");
        let p = dir.to_str().unwrap();
        let code = format!(r#"chdir("{p}"); my @d = glob("**(/)"); join(",", sort @d)"#);
        let out = eval_locked(&code).to_string();
        assert_eq!(out, "sub,sub/deeper", "got {out}");
        assert!(!out.contains("file1.txt"), "got {out}");
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn glob_dot_qualifier_lists_regular_files_recursively() {
    with_global_flags(|| {
        let dir = fixture_dir("globdot");
        let p = dir.to_str().unwrap();
        let code = format!(r#"chdir("{p}"); my @f = glob("**(.)"); join(",", sort @f)"#);
        let out = eval_locked(&code).to_string();
        for f in ["file1.txt", "file2.txt", "sub/file3.txt", "file4.txt"] {
            assert!(out.contains(f), "missing {f}: {out}");
        }
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn glob_n_qualifier_returns_empty_on_no_match() {
    with_global_flags(|| {
        let dir = fixture_dir("globn");
        let p = dir.to_str().unwrap();
        let code =
            format!(r#"chdir("{p}"); my @f = glob("nope-this-cannot-match*(N)"); scalar(@f)"#);
        assert_eq!(eval_int_locked(&code), 0);
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
fn glob_size_qualifier_filters_by_byte_count() {
    with_global_flags(|| {
        let dir = fixture_dir("globsize");
        // file1.txt is 6 bytes ("alpha\n"); a 0-byte file shouldn't match (L+1).
        std::fs::write(dir.join("empty"), "").unwrap();
        let p = dir.to_str().unwrap();
        let code = format!(
            r#"chdir("{p}"); my @f = glob("**(L+1)"); my $hit = 0; for (@f) {{ $hit = 1 if /file1\.txt/ }} $hit"#
        );
        assert_eq!(eval_int_locked(&code), 1);
        let code_neg = format!(
            r#"chdir("{p}"); my @f = glob("**(L+1)"); my $hit = 0; for (@f) {{ $hit = 1 if /empty/ }} $hit"#
        );
        assert_eq!(eval_int_locked(&code_neg), 0);
        std::fs::remove_dir_all(&dir).ok();
    });
}

// Regression: zshrs 0.11.47's `glob_path` only matches glob-TOKENIZED
// metacharacters, so a raw `*` reached the matcher as a literal and the pattern
// echoed back instead of expanding (and `haswilds` on the raw pattern returned
// false, so the glob was misclassified as a plain path upstream). A plain
// (qualifier-free) wildcard must expand to the real entries — see
// `perl_fs::stryke_glob` / `raw_haswilds`.
#[test]
fn glob_plain_wildcard_expands_not_echoes() {
    with_global_flags(|| {
        let dir = fixture_dir("plainwild");
        let p = dir.to_str().unwrap();
        let code = format!(r#"chdir("{p}"); my @f = glob("*.txt"); join(",", sort @f)"#);
        let out = eval_locked(&code).to_string();
        assert_eq!(out, "file1.txt,file2.txt", "got {out}");
        assert!(
            !out.contains('*'),
            "pattern echoed instead of expanded: {out}"
        );
        std::fs::remove_dir_all(&dir).ok();
    });
}

// Regression companion: a plain wildcard with no match yields an empty list
// (Perl nullglob), not the literal pattern echoed back. 0.11.47's
// `globdata_glob` dropped the `gf_nullglob` empty-result guard, so the
// suppression lives in `perl_fs::stryke_glob`.
#[test]
fn glob_plain_wildcard_no_match_is_empty() {
    with_global_flags(|| {
        let dir = fixture_dir("plainwildempty");
        let p = dir.to_str().unwrap();
        let code = format!(r#"chdir("{p}"); my @f = glob("*.nomatch-xyz"); scalar(@f)"#);
        assert_eq!(eval_int_locked(&code), 0);
        std::fs::remove_dir_all(&dir).ok();
    });
}
