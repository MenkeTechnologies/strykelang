use crate::common::*;
use perlrs::interpreter::Interpreter;

#[test]
fn array_ref() {
    assert_eq!(eval_int("my $r = [1,2,3]; $r->[1]"), 2);
}

#[test]
fn hash_ref() {
    assert_eq!(eval_int("my $r = {a => 1, b => 2}; $r->{b}"), 2);
}

#[test]
fn defined_undef() {
    assert_eq!(eval_int("defined(42)"), 1);
    assert_eq!(eval_int("defined(undef)"), 0);
}

#[test]
fn ref_type() {
    assert_eq!(eval_string(r#"ref([])"#), "ARRAY");
    assert_eq!(eval_string(r#"ref({})"#), "HASH");
    assert_eq!(eval_string(r#"ref(\42)"#), "SCALAR");
}

#[test]
fn bless_ref_type() {
    assert_eq!(eval_string(r#"ref(bless({}, "MyClass"))"#), "MyClass");
}

#[test]
fn eval_string_code() {
    assert_eq!(eval_int(r#"eval("2 + 2")"#), 4);
}

#[test]
fn wantarray_undef() {
    assert_eq!(eval_int("wantarray"), 0);
}

#[test]
fn caller_builtin() {
    assert_eq!(
        eval_string(r#"join(",", caller())"#),
        "main,-e,1"
    );
}

#[test]
fn package_sets_package_glob() {
    assert_eq!(
        eval_string(r#"package Foo::Bar; $__PACKAGE__"#),
        "Foo::Bar"
    );
}

#[test]
fn use_strict_noop() {
    assert_eq!(eval_int("use strict; 1"), 1);
}

#[test]
fn numeric_functions() {
    assert_eq!(eval_int("abs(-5)"), 5);
    assert_eq!(eval_int("int(3.7)"), 3);
    assert_eq!(eval_int("hex('ff')"), 255);
    assert_eq!(eval_int("oct('77')"), 63);
    assert_eq!(eval_string("chr(65)"), "A");
    assert_eq!(eval_int("ord('A')"), 65);
}

#[test]
fn die_in_eval() {
    let code = r#"eval { die "test error\n" }; $@ eq "test error\n" ? 1 : 0"#;
    let program = perlrs::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    let result = interp.execute(&program);
    assert!(result.is_ok());
}
