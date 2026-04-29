//! Nested references, type juggling, and aggregate edge cases.

use crate::common::*;

#[test]
fn nested_array_refs_arrow_chain() {
    assert_eq!(eval_int("my $a = [[1,2],[3,4]]; $a->[0]->[1]"), 2);
}

#[test]
fn hash_ref_to_hash_arrow_chain() {
    assert_eq!(eval_int("my %h = (a => { b => 99 }); $h{a}->{b}"), 99);
}

#[test]
fn numeric_equality_coerces_string_operand() {
    assert_eq!(eval_int(r#"7 == "7""#), 1);
    assert_eq!(eval_int(r#"7 == "8""#), 0);
}

#[test]
fn empty_array_in_list_context() {
    assert_eq!(eval_int("my @a = (); len(@a)"), 0);
}

#[test]
fn string_repeat_zero_is_empty() {
    assert_eq!(eval_string(r#""x" x 0"#), "");
}

#[test]
fn empty_hash_has_zero_keys() {
    assert_eq!(eval_int(r#"my %h = (); len(keys %h)"#), 0);
}
