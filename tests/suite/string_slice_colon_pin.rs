//! Pin stryke's bracket-`:` string slicing per `docs/STYLE_GUIDE.md`
//! §6a: `$s[N]` for char-index, `$s[N:M]` for closed-inclusive slice,
//! negative indices for tail offsets, clamping past the end.
//! Probed against the running interpreter on 2026-05-23.
//!
//! Stryke `[N:M]` is **inclusive on both ends** — `"hello"[1:3]` is
//! `"ell"` (chars 1, 2, 3). This differs from Perl's `substr($s, 1, 3)`
//! which is start-plus-length, even when the example output coincides.

use crate::common::*;

#[test]
fn string_index_single_char() {
    let code = r#"
        my $s = "hello";
        $s[1] eq "e" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_index_first_char() {
    let code = r#"
        my $s = "stryke";
        $s[0] eq "s" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_index_negative_returns_last() {
    let code = r#"
        my $s = "hello";
        $s[-1] eq "o" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_index_negative_two_returns_penultimate() {
    let code = r#"
        my $s = "hello";
        $s[-2] eq "l" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_closed_inclusive_both_ends() {
    // $s[1:3] = "ell" — chars 1, 2, 3 inclusive.
    let code = r#"
        my $s = "hello";
        $s[1:3] eq "ell" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_single_index_via_equal_range() {
    // $s[1:1] = just char 1.
    let code = r#"
        my $s = "hello";
        $s[1:1] eq "e" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_full_range_returns_whole_string() {
    let code = r#"
        my $s = "stryke";
        $s[0:5] eq "stryke" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_negative_range_returns_tail() {
    // $s[-3:-1] = last three chars.
    let code = r#"
        my $s = "hello";
        $s[-3:-1] eq "llo" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_past_end_clamps_gracefully() {
    // $s[0:99] clamps to actual end — no warn, no panic.
    let code = r#"
        my $s = "hello";
        $s[0:99] eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_index_on_empty_string_yields_empty() {
    let code = r#"
        my $s = "";
        $s[0] eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_consistent_with_substr_for_basic_case() {
    // `[N:M]` (inclusive) vs `substr(s, N, len)` should agree when
    // the inclusive count and `substr` length are the same.
    let code = r#"
        my $s = "abcdefgh";
        # [2:4] = "cde" (3 chars), substr($s, 2, 3) = "cde".
        ($s[2:4] eq substr($s, 2, 3)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_index_chain_via_pipe_forward() {
    // First char of the uppercased reversed string of "stryke".
    let code = r#"
        my $first = "stryke" |> rev |> uc;
        $first[0] eq "E" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_slice_negative_to_negative_one() {
    // Middle three chars via -4:-2.
    let code = r#"
        my $s = "abcdef";
        $s[-4:-2] eq "cde" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
