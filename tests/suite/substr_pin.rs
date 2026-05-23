//! Pin `substr` semantics: 2-arg (offset only), 3-arg (offset +
//! length), 4-arg replacement, negative offsets and lengths, edge
//! cases beyond string boundaries. Probed against the running
//! interpreter on 2026-05-23 before pinning.

use crate::common::*;

#[test]
fn substr_two_arg_offset_only_returns_tail() {
    let code = r#"
        my $s = "hello world";
        substr($s, 6) eq "world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_three_arg_with_length() {
    let code = r#"
        my $s = "hello world";
        substr($s, 0, 5) eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_negative_offset_counts_from_end() {
    let code = r#"
        my $s = "hello";
        substr($s, -3) eq "llo" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_negative_offset_with_length() {
    let code = r#"
        my $s = "hello";
        substr($s, -3, 2) eq "ll" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_negative_length_trims_from_end() {
    // Negative length = take from $start up to ($len + neg) of end.
    let code = r#"
        my $s = "hello";
        substr($s, 2, -1) eq "ll" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_length_overshoot_clamps_to_end() {
    let code = r#"
        my $s = "hello";
        substr($s, 0, 100) eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_offset_beyond_end_yields_empty() {
    let code = r#"
        my $s = "hello";
        my $r = substr($s, 10);
        $r eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_zero_length_yields_empty() {
    let code = r#"
        my $s = "hello";
        substr($s, 0, 0) eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_four_arg_replaces_in_place() {
    let code = r#"
        my $s = "hello world";
        substr($s, 0, 5, "HOWDY");
        $s eq "HOWDY world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_four_arg_replace_tail() {
    let code = r#"
        my $s = "hello world";
        substr($s, 6, 5, "perl");
        $s eq "hello perl" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_four_arg_inserts_when_length_zero() {
    let code = r#"
        my $s = "hello";
        substr($s, 2, 0, "INS");
        $s eq "heINSllo" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_four_arg_deletes_when_replacement_empty() {
    let code = r#"
        my $s = "hello";
        substr($s, 1, 3, "");
        $s eq "ho" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_four_arg_replacement_changes_length() {
    // Replace 1 char with 3 chars; string grows by 2.
    let code = r#"
        my $s = "abc";
        substr($s, 1, 1, "XYZ");
        ($s eq "aXYZc" && len($s) == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_roundtrip_via_explicit_split_join() {
    // substr-then-rejoin must reconstruct the original.
    let code = r#"
        my $s = "the quick brown fox";
        my $head = substr($s, 0, 9);   # "the quick"
        my $tail = substr($s, 9);      # " brown fox"
        ($head . $tail) eq $s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
