use crate::common::*;

#[test]
fn basic_sub() {
    assert_eq!(
        eval_int("sub add { my $a = shift @_; my $b = shift @_; return $a + $b; } add(3, 4)"),
        7
    );
}

#[test]
fn recursive_fibonacci() {
    assert_eq!(
        eval_int("sub fib { my $n = shift @_; return $n if $n <= 1; return fib($n-1) + fib($n-2); } fib(10)"),
        55
    );
}

#[test]
fn return_exits_sub_before_following_statement() {
    assert_eq!(
        eval_int(
            "sub f { \
                 if (1) { return 3; } \
                 9 \
             } \
             f()",
        ),
        3
    );
}

#[test]
fn return_with_postfix_if() {
    assert_eq!(
        eval_int("sub f { my $n = shift @_; return 0 if $n <= 0; return $n; } f(5)"),
        5
    );
    assert_eq!(
        eval_int("sub f { my $n = shift @_; return 0 if $n <= 0; return $n; } f(-1)"),
        0
    );
}

#[test]
fn sub_with_prototype_two_scalars_uses_at_underscore() {
    assert_eq!(
        eval_int("sub add2 ($$) { return $_[0] + $_[1]; } add2(40, 2)"),
        42
    );
}

#[test]
fn coderef_invocation_with_arrow() {
    assert_eq!(
        eval_int(r#"sub dbl { $_[0] * 2 } my $r = \&dbl; $r->(21)"#),
        42,
    );
}
