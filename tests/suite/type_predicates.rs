use crate::common::*;

#[test]
fn predicate_is_array() {
    assert_eq!(eval_int("is_array([])"), 1);
    assert_eq!(eval_int("is_arrayref([])"), 1);
    assert_eq!(eval_int("is_array({})"), 0);
    assert_eq!(eval_int("is_array(42)"), 0);
}

#[test]
fn predicate_is_hash() {
    assert_eq!(eval_int("is_hash({})"), 1);
    assert_eq!(eval_int("is_hashref({})"), 1);
    assert_eq!(eval_int("is_hash([])"), 0);
    assert_eq!(eval_int("is_hash(42)"), 0);
}

#[test]
fn predicate_is_code() {
    assert_eq!(eval_int("is_code(sub {})"), 1);
    assert_eq!(eval_int("is_coderef(sub {})"), 1);
    assert_eq!(eval_int("is_code([])"), 0);
}

#[test]
fn predicate_is_ref() {
    assert_eq!(eval_int("is_ref([])"), 1);
    assert_eq!(eval_int("is_ref({})"), 1);
    assert_eq!(eval_int("is_ref(sub {})"), 1);
    assert_eq!(eval_int("is_ref(42)"), 0);
}

#[test]
fn predicate_is_undef() {
    assert_eq!(eval_int("is_undef(undef)"), 1);
    assert_eq!(eval_int("is_undef(0)"), 0);
    assert_eq!(eval_int("is_undef('')"), 0);
}

#[test]
fn predicate_is_defined() {
    assert_eq!(eval_int("is_defined(42)"), 1);
    assert_eq!(eval_int("is_def(42)"), 1);
    assert_eq!(eval_int("is_defined(undef)"), 0);
    assert_eq!(eval_int("is_def(undef)"), 0);
}

#[test]
fn predicate_is_string() {
    assert_eq!(eval_int("is_string('abc')"), 1);
    assert_eq!(eval_int("is_str('abc')"), 1);
    assert_eq!(eval_int("is_string('42')"), 1);
    assert_eq!(eval_int("is_string(42)"), 0);
}

#[test]
fn predicate_is_int() {
    assert_eq!(eval_int("is_int(42)"), 1);
    assert_eq!(eval_int("is_integer(42)"), 1);
    assert_eq!(eval_int("is_int(3.14)"), 0);
    assert_eq!(eval_int("is_int('42')"), 0); // it's a string, not an internal int
}

#[test]
fn predicate_is_float() {
    assert_eq!(eval_int("is_float(3.14)"), 1);
    assert_eq!(eval_int("is_float(42)"), 0);
}
