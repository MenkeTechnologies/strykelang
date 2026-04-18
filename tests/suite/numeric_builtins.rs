use crate::common::*;

#[test]
fn builtin_gcd_basic() {
    assert_eq!(eval_int("gcd(12, 18)"), 6);
    assert_eq!(eval_int("gcd(101, 103)"), 1);
    assert_eq!(eval_int("gcd(0, 5)"), 5);
    assert_eq!(eval_int("gcd(5, 0)"), 5);
    assert_eq!(eval_int("gcd(0, 0)"), 0);
}

#[test]
fn builtin_gcd_negative() {
    assert_eq!(eval_int("gcd(-12, 18)"), 6);
    assert_eq!(eval_int("gcd(12, -18)"), 6);
    assert_eq!(eval_int("gcd(-12, -18)"), 6);
}

#[test]
fn builtin_gcd_topic() {
    assert_eq!(eval_int("$_ = 12; gcd()"), 12);
}

#[test]
fn builtin_lcm_basic() {
    assert_eq!(eval_int("lcm(12, 18)"), 36);
    assert_eq!(eval_int("lcm(10, 5)"), 10);
    assert_eq!(eval_int("lcm(7, 3)"), 21);
    assert_eq!(eval_int("lcm(0, 5)"), 0);
    assert_eq!(eval_int("lcm(5, 0)"), 0);
}

#[test]
fn builtin_lcm_negative() {
    assert_eq!(eval_int("lcm(-12, 18)"), 36);
    assert_eq!(eval_int("lcm(12, -18)"), 36);
    assert_eq!(eval_int("lcm(-12, -18)"), 36);
}

#[test]
fn builtin_lcm_topic() {
    assert_eq!(eval_int("$_ = 12; lcm()"), 12);
}

#[test]
fn builtin_factorial_basic() {
    assert_eq!(eval_int("factorial(0)"), 1);
    assert_eq!(eval_int("factorial(1)"), 1);
    assert_eq!(eval_int("factorial(5)"), 120);
    assert_eq!(eval_int("fact(6)"), 720);
}

#[test]
fn builtin_factorial_negative() {
    assert_eq!(eval_string("defined(factorial(-1)) ? 1 : 0"), "0");
}

#[test]
fn builtin_fibonacci_basic() {
    assert_eq!(eval_int("fibonacci(0)"), 0);
    assert_eq!(eval_int("fibonacci(1)"), 1);
    assert_eq!(eval_int("fibonacci(2)"), 1);
    assert_eq!(eval_int("fibonacci(3)"), 2);
    assert_eq!(eval_int("fibonacci(10)"), 55);
    assert_eq!(eval_int("fib(10)"), 55);
}

#[test]
fn builtin_is_prime_basic() {
    assert_eq!(eval_int("is_prime(2)"), 1);
    assert_eq!(eval_int("is_prime(3)"), 1);
    assert_eq!(eval_int("is_prime(4)"), 0);
    assert_eq!(eval_int("is_prime(17)"), 1);
    assert_eq!(eval_int("is_prime(100)"), 0);
    assert_eq!(eval_int("is_prime(101)"), 1);
}

#[test]
fn builtin_is_prime_small() {
    assert_eq!(eval_int("is_prime(0)"), 0);
    assert_eq!(eval_int("is_prime(1)"), 0);
    assert_eq!(eval_int("is_prime(-7)"), 0);
}

#[test]
fn builtin_is_square_basic() {
    assert_eq!(eval_int("is_square(0)"), 1);
    assert_eq!(eval_int("is_square(1)"), 1);
    assert_eq!(eval_int("is_square(4)"), 1);
    assert_eq!(eval_int("is_square(9)"), 1);
    assert_eq!(eval_int("is_square(2)"), 0);
    assert_eq!(eval_int("is_square(10)"), 0);
    assert_eq!(eval_int("is_square(-4)"), 0);
}
