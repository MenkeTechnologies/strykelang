//! Pin case-conversion builtins: `lc`, `uc`, `ucfirst`, `lcfirst`.
//! Probed against the running interpreter on 2026-05-23 before
//! pinning — every assertion mirrors observed output.

use crate::common::*;

#[test]
fn lc_lowers_only_alphabetic() {
    let code = r#"
        lc("HELLO World!") eq "hello world!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uc_raises_only_alphabetic() {
    let code = r#"
        uc("hello world!") eq "HELLO WORLD!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lc_on_already_lower_is_identity() {
    let code = r#"
        lc("abc 123") eq "abc 123" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uc_on_already_upper_is_identity() {
    let code = r#"
        uc("ABC 123") eq "ABC 123" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ucfirst_titles_first_char_only() {
    let code = r#"
        ucfirst("hello world") eq "Hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lcfirst_lowers_first_char_only() {
    let code = r#"
        lcfirst("HELLO WORLD") eq "hELLO WORLD" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ucfirst_leaves_internal_caps_intact() {
    let code = r#"
        ucfirst("aBc DeF") eq "ABc DeF" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lc_empty_string_stays_empty() {
    let code = r#"
        lc("") eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uc_empty_string_stays_empty() {
    let code = r#"
        uc("") eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ucfirst_empty_string_stays_empty() {
    let code = r#"
        ucfirst("") eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uc_leaves_digits_and_punctuation_unchanged() {
    let code = r#"
        uc("123abc!@#") eq "123ABC!@#" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lc_uc_roundtrip_on_lower_input() {
    let code = r#"
        my $s = "hello";
        lc(uc($s)) eq $s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ucfirst_then_lcfirst_restores_lowercase_start() {
    let code = r#"
        my $s = "hello WORLD";
        lcfirst(ucfirst($s)) eq $s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
