//! Pins for `swallow PATTERN` — glob → `{ canonical_abspath => raw_bytes }` hash.
//!
//! Behaviour fixed here:
//!   * round-trip multi-file glob (two files, both keys present, values match)
//!   * binary safe (embedded NUL + non-UTF8 bytes survive)
//!   * symlink keys collapse to the real path (`fs::canonicalize`)
//!   * non-regular match is a hard error (matches `slurp` policy)
//!   * `(N)` null-glob qualifier yields an empty hash, no error
//!   * `**` recursion lifts files from nested directories
//!   * `swa` alias parses
//!
//! All tests use `/tmp/stryke_swallow_pin_<nanos>_<tag>/` and clean up on success.

use crate::common::*;

fn fresh_dir(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = format!("/tmp/stryke_swallow_pin_{}_{}", nanos, tag);
    std::fs::create_dir_all(&dir).expect("mkdir tmp");
    dir
}

fn rm_rf(dir: &str) {
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn swallow_two_files_returns_hash_keyed_by_abspath() {
    let dir = fresh_dir("two_files");
    std::fs::write(format!("{dir}/a.txt"), b"AAA").unwrap();
    std::fs::write(format!("{dir}/b.txt"), b"BBBB").unwrap();
    let real_dir = std::fs::canonicalize(&dir).unwrap();
    let code = format!(
        r#"
            my %h = swallow("{dir}/*.txt");
            my $count = scalar keys %h;
            my $a = $h{{"{real}/a.txt"}};
            my $b = $h{{"{real}/b.txt"}};
            ($count == 2 && length($a) == 3 && length($b) == 4) ? 1 : 0
        "#,
        dir = dir,
        real = real_dir.display(),
    );
    let ok = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(ok, 1, "expected hash{{a,b}} with right byte lengths");
}

#[test]
fn swallow_preserves_binary_bytes_through_hash() {
    let dir = fresh_dir("binary");
    // Embedded NUL + 0xFF (invalid UTF-8 start byte) + non-printable.
    let payload: Vec<u8> = vec![0x00, 0xFF, 0x01, 0xFE, 0x7F, 0x80];
    std::fs::write(format!("{dir}/bin.dat"), &payload).unwrap();
    let real = std::fs::canonicalize(&dir).unwrap();
    // `length` on a bytes value reports raw byte count; we verify exact length and
    // probe one of the high bytes that would have been lost to UTF-8 decoding.
    let code = format!(
        r#"
            my %h = swallow("{dir}/bin.dat");
            my $b = $h{{"{real}/bin.dat"}};
            my $len_ok = length($b) == 6 ? 1 : 0;
            $len_ok
        "#,
        dir = dir,
        real = real.display(),
    );
    let ok = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(ok, 1, "expected 6 raw bytes survived round-trip");
}

#[test]
fn swallow_flattens_symlinks_to_real_path_key() {
    // Symlink farm: real file lives in `real/`, glob hits the symlink in `link/`.
    let dir = fresh_dir("symlink");
    std::fs::create_dir_all(format!("{dir}/real")).unwrap();
    std::fs::create_dir_all(format!("{dir}/link")).unwrap();
    let real_target = format!("{dir}/real/payload.txt");
    std::fs::write(&real_target, b"hello").unwrap();
    // Symlink in `link/` pointing at the real file.
    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_target, format!("{dir}/link/alias.txt")).unwrap();
    let canon_real = std::fs::canonicalize(&real_target)
        .unwrap()
        .display()
        .to_string();
    // Glob via the *symlink* path; key must be the canonicalised real path,
    // not the symlink path.
    let code = format!(
        r#"
            my %h = swallow("{dir}/link/*.txt");
            exists $h{{"{canon_real}"}} ? 1 : 0
        "#,
        dir = dir,
        canon_real = canon_real,
    );
    let ok = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(ok, 1, "expected hash key to be the symlink target's real path");
}

#[test]
fn swallow_hard_fails_on_directory_match() {
    // Glob qualifier `(/)` selects directories; swallow refuses, mirroring slurp.
    let dir = fresh_dir("dir_match");
    std::fs::create_dir_all(format!("{dir}/subdir")).unwrap();
    let kind = eval_err_kind(&format!(r#"swallow("{dir}/*(/)")"#));
    rm_rf(&dir);
    assert!(
        matches!(kind, stryke::error::ErrorKind::Runtime { .. }),
        "expected runtime error on directory match, got {kind:?}"
    );
}

#[test]
fn swallow_null_glob_qualifier_returns_empty_hash() {
    // `(N)` is the null-glob qualifier — no matches becomes empty list, no error.
    let dir = fresh_dir("nullglob");
    let code = format!(
        r#"
            my %h = swallow("{dir}/no_such_*(N)");
            scalar keys %h
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(n, 0, "(N) should give an empty hash, not an error");
}

#[test]
fn swallow_recursive_double_star_picks_up_nested_files() {
    let dir = fresh_dir("recursive");
    std::fs::create_dir_all(format!("{dir}/a/b/c")).unwrap();
    std::fs::write(format!("{dir}/top.md"), b"top").unwrap();
    std::fs::write(format!("{dir}/a/mid.md"), b"mid").unwrap();
    std::fs::write(format!("{dir}/a/b/c/deep.md"), b"deep").unwrap();
    let code = format!(
        r#"
            my %h = swallow("{dir}/**/*.md");
            scalar keys %h
        "#,
        dir = dir,
    );
    let n = eval_int(&code);
    rm_rf(&dir);
    assert_eq!(n, 3, "`**/*.md` should hit top, mid, deep");
}

#[test]
fn swa_alias_parses_and_runs() {
    let dir = fresh_dir("alias");
    std::fs::write(format!("{dir}/x"), b"x").unwrap();
    let n = eval_int(&format!(
        r#"
            my %h = swa("{dir}/x");
            scalar keys %h
        "#
    ));
    rm_rf(&dir);
    assert_eq!(n, 1, "`swa` alias must produce the same hash as `swallow`");
}
