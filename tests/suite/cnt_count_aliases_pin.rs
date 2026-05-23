//! Pin the `cnt` and `count` aliases for `len` per
//! `docs/STYLE_GUIDE.md` §5. All three names point at the same
//! builtin; the style guide picks them by context-readability rather
//! than semantics. Probed against the running interpreter on
//! 2026-05-23.

use crate::common::*;

#[test]
fn cnt_of_string_equals_len_of_string() {
    let code = r#"
        my $s = "stryke";
        (cnt($s) == 6 && cnt($s) == len($s)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_of_string_equals_len_of_string() {
    let code = r#"
        my $s = "hello world";
        (count($s) == 11 && count($s) == len($s)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cnt_of_array_equals_len_of_array() {
    let code = r#"
        my @a = (10, 20, 30, 40, 50);
        (cnt(@a) == 5 && cnt(@a) == len(@a)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_of_array_equals_len_of_array() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5, 6, 7);
        (count(@a) == 7 && count(@a) == len(@a)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cnt_of_hash_returns_key_count() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        cnt(%h) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_of_hash_returns_key_count() {
    let code = r#"
        my %h = (x => 10, y => 20);
        count(%h) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cnt_count_len_agree_on_empty_array() {
    let code = r#"
        my @a;
        (len(@a) == 0 && cnt(@a) == 0 && count(@a) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cnt_count_len_agree_on_empty_string() {
    let code = r#"
        my $s = "";
        (len($s) == 0 && cnt($s) == 0 && count($s) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cnt_of_arrayref_via_deref() {
    let code = r#"
        my $r = [10, 20, 30];
        cnt(@$r) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cnt_as_pipe_forward_tail() {
    // Style-guide idiom: `pgrep { … } |> cnt` reads as "count".
    let code = r#"
        my @evens = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10) |> grep { _ % 2 == 0 };
        cnt(@evens) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_in_pipe_forward() {
    let code = r#"
        my $n = "the quick brown fox" |> split(/\s+/) |> count;
        $n == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
