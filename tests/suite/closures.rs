//! Anonymous subs and lexical capture.

use crate::common::*;

#[test]
fn anon_sub_captures_outer_lexical() {
    assert_eq!(
        eval_int(
            "my $x = 10; \
             my $c = fn { $x + 5 }; \
             $c->()",
        ),
        15
    );
}

#[test]
fn anon_sub_captures_lexical_array_and_hash() {
    assert_eq!(
        eval_int(
            "my @a = (10, 20, 30); \
             my $c = fn { $a[1] }; \
             $c->()",
        ),
        20
    );
    assert_eq!(
        eval_int(
            "my %h = (k => 42); \
             my $c = fn { $h{k} }; \
             $c->()",
        ),
        42
    );
}

#[test]
fn sub_implicit_return_last_expression() {
    assert_eq!(eval_int("fn foo { 5 } foo()"), 5);
}

#[test]
fn named_sub_captures_outer_lexical_vm_and_tree() {
    assert_eq!(
        eval_int(
            "my $x = 10; \
             fn foo { $x + 5 } \
             foo()",
        ),
        15
    );
}
