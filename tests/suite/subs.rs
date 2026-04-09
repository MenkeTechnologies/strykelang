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
