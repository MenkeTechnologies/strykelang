use crate::common::*;

#[test]
fn string_is_empty() {
    assert_eq!(eval_int("is_empty('')"), 1);
    assert_eq!(eval_int("is_empty('abc')"), 0);
    assert_eq!(eval_int("is_empty(undef)"), 1);
}

#[test]
fn string_is_blank() {
    assert_eq!(eval_int("is_blank('')"), 1);
    assert_eq!(eval_int("is_blank('  ')"), 1);
    assert_eq!(eval_int("is_blank('\t\n')"), 1);
    assert_eq!(eval_int("is_blank(' a ')"), 0);
    assert_eq!(eval_int("is_blank(undef)"), 1);
}

#[test]
fn string_is_numeric() {
    assert_eq!(eval_int("is_numeric('123')"), 1);
    assert_eq!(eval_int("is_numeric('12.3')"), 1);
    assert_eq!(eval_int("is_numeric('-1.2e3')"), 1);
    assert_eq!(eval_int("is_numeric('abc')"), 0);
    assert_eq!(eval_int("is_numeric('')"), 0);
}

#[test]
fn string_case_predicates() {
    assert_eq!(eval_int("is_upper('ABC')"), 1);
    assert_eq!(eval_int("is_upper('AbC')"), 0);
    assert_eq!(eval_int("is_lower('abc')"), 1);
    assert_eq!(eval_int("is_lower('aBc')"), 0);
    assert_eq!(eval_int("is_alpha('abcABC')"), 1);
    assert_eq!(eval_int("is_alpha('abc1')"), 0);
    assert_eq!(eval_int("is_digit('123')"), 1);
    assert_eq!(eval_int("is_digit('12a')"), 0);
    assert_eq!(eval_int("is_alnum('abc123')"), 1);
    assert_eq!(eval_int("is_alnum('abc 123')"), 0);
}

#[test]
fn string_space_predicates() {
    assert_eq!(eval_int("is_space(' ')"), 1);
    assert_eq!(eval_int("is_space('\t\n\r ')"), 1);
    assert_eq!(eval_int("is_space('a')"), 0);
}

#[test]
fn string_search_helpers() {
    assert_eq!(eval_int("starts_with('hello world', 'hello')"), 1);
    assert_eq!(eval_int("sw('hello world', 'hello')"), 1);
    assert_eq!(eval_int("ends_with('hello world', 'world')"), 1);
    assert_eq!(eval_int("ew('hello world', 'world')"), 1);
    assert_eq!(eval_int("contains('hello world', 'o w')"), 1);
    assert_eq!(eval_int("contains('hello world', 'xyz')"), 0);
}

#[test]
fn string_transform_helpers() {
    assert_eq!(eval_string("capitalize('hello')"), "Hello");
    assert_eq!(eval_string("cap('world')"), "World");
    assert_eq!(eval_string("swap_case('aBcD')"), "AbCd");
    assert_eq!(eval_string("repeat('abc', 3)"), "abcabcabc");
    assert_eq!(eval_string("title_case('hello world')"), "Hello World");
    assert_eq!(eval_string("squish('  hello   world  ')"), "hello world");
}

#[test]
fn string_padding_helpers() {
    assert_eq!(eval_string("pad_left('abc', 5)"), "  abc");
    assert_eq!(eval_string("lpad('abc', 5, 'x')"), "xxabc");
    assert_eq!(eval_string("pad_right('abc', 5)"), "abc  ");
    assert_eq!(eval_string("rpad('abc', 5, 'y')"), "abcyy");
    assert_eq!(eval_string("center('abc', 7)"), "  abc  ");
    assert_eq!(eval_string("center('abc', 7, '=')"), "==abc==");
}

#[test]
fn string_truncate_and_reverse() {
    assert_eq!(eval_string("truncate_at('hello world', 5)"), "hell\u{2026}");
    assert_eq!(eval_string("shorten('hello world', 8)"), "hello w\u{2026}");
    assert_eq!(eval_string("reverse_str('abc')"), "cba");
    assert_eq!(eval_string("rev_str('hello')"), "olleh");
}
