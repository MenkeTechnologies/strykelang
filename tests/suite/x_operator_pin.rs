//! Pin the `x` repetition operator: scalar string repeat, list
//! repeat in list context, zero / negative counts, repetition of a
//! parenthesised list. Probed against the running interpreter on
//! 2026-05-23 before pinning.

use crate::common::*;

#[test]
fn x_string_repeat_basic() {
    let code = r#"
        my $s = "ab" x 3;
        $s eq "ababab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_string_repeat_zero_is_empty() {
    let code = r#"
        my $s = "abc" x 0;
        ($s eq "" && len($s) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_string_repeat_negative_is_empty() {
    // Perl-compatible: count < 0 produces "".
    let code = r#"
        my $s = "abc" x -5;
        $s eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_string_repeat_one_returns_original() {
    let code = r#"
        my $s = "hello" x 1;
        $s eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_list_repeat_in_list_context_flattens() {
    // `(0) x 5` in list context expands to five zeros.
    let code = r#"
        my @r = (0) x 5;
        len(@r) == 5 && $r[0] == 0 && $r[4] == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_list_repeat_multi_element_pair() {
    let code = r#"
        my @r = (1, 2) x 3;
        len(@r) == 6 && join(",", @r) eq "1,2,1,2,1,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_list_repeat_zero_yields_empty_list() {
    let code = r#"
        my @r = (1, 2, 3) x 0;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_string_repeat_large_count_length_correct() {
    // 1_000 copies of a 3-char string → 3_000 chars.
    let code = r#"
        my $s = "abc" x 1000;
        len($s) == 3000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_array_preallocation_idiom() {
    // Classic idiom for pre-allocating a zero-filled array.
    let code = r#"
        my @grid = (0) x 16;
        my $sum = 0;
        for my $v (@grid) { $sum += $v }
        ($sum == 0 && len(@grid) == 16) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn x_string_repeat_combines_with_concat() {
    let code = r#"
        my $banner = "=" x 5 . " title " . "=" x 5;
        $banner eq "===== title =====" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
