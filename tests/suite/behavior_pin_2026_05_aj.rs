//! Behavior-pinning batch AI (2026-05-05): More Numeric Builtins

use crate::common::*;

#[test]
fn builtin_gcd_more() {
    assert_eq!(eval_int("gcd(99, 121)"), 11);
    assert_eq!(eval_int("gcd(1024, 256)"), 256);
}

#[test]
fn builtin_lcm_more() {
    assert_eq!(eval_int("lcm(10, 15)"), 30);
    assert_eq!(eval_int("lcm(7, 11)"), 77);
}

#[test]
fn builtin_factorial_large() {
    assert_eq!(eval_int("factorial(10)"), 3628800);
}

#[test]
fn builtin_fibonacci_large() {
    assert_eq!(eval_int("fib(20)"), 6765);
}

#[test]
fn builtin_is_prime_more() {
    assert_eq!(eval_int("is_prime(97)"), 1);
    assert_eq!(eval_int("is_prime(111)"), 0);
}

#[test]
fn builtin_sum() {
    assert_eq!(eval_int("sum 1..10"), 55);
    assert_eq!(eval_int("my @a = (10, 20, 30); sum @a"), 60);
}

#[test]
fn builtin_product() {
    assert_eq!(eval_int("product 1..5"), 120);
    assert_eq!(eval_int("my @a = (1, 2, 3, 4, 5); product @a"), 120);
    assert_eq!(eval_int("product(10, 20)"), 200);
}
