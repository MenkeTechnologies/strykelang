//! Behavior-pinning batch BD (2026-05-08): Sweeping zsh glob qualifiers documented in README.md.

use crate::common::*;

#[test]
fn zsh_glob_null_glob_qualifier_works() {
    let out = eval_string(r#"len(glob("doesnotexist_abc123*(N)"))"#);
    assert_eq!(out, "0", "expected null glob to return 0 elements");
}

#[test]
fn zsh_glob_sort_and_slice_qualifier_works() {
    // (om[1]) means sort by mtime desc (o=sort, m=mtime), take 1.
    // It should return exactly 1 file if there are matching files.
    let out = eval_string(r#"my @f = glob("strykelang/*.rs(om[1])"); join(",", @f)"#);
    let count = out.split(',').filter(|s| !s.is_empty()).count();
    assert_eq!(
        count, 1,
        "expected slicing glob qualifier to return exactly 1 element, got: {}",
        out
    );
}

#[test]
fn zsh_glob_type_qualifier_works() {
    // (/) means directories only
    let out = eval_string(r#"my @dirs = glob("strykelang(/)"); len(@dirs)"#);
    assert_eq!(out, "1", "expected type qualifier to find the directory");
}
