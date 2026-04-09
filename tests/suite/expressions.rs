use crate::common::*;

#[test]
fn integer_arithmetic() {
    assert_eq!(eval_int("3 + 4"), 7);
    assert_eq!(eval_int("10 - 3"), 7);
    assert_eq!(eval_int("6 * 7"), 42);
    assert_eq!(eval_int("15 / 3"), 5);
    assert_eq!(eval_int("17 % 5"), 2);
    assert_eq!(eval_int("2 ** 10"), 1024);
}

#[test]
fn float_literal_arithmetic() {
    assert_eq!(eval_int("3.5 + 1.5"), 5);
    assert_eq!(eval_string("3.0 + 0.5"), "3.5");
}

#[test]
fn operator_precedence() {
    assert_eq!(eval_int("2 + 3 * 4"), 14);
    assert_eq!(eval_int("(2 + 3) * 4"), 20);
    assert_eq!(eval_int("2 ** 3 ** 2"), 512);
}

#[test]
fn comparison_operators() {
    assert_eq!(eval_int("5 == 5"), 1);
    assert_eq!(eval_int("5 != 3"), 1);
    assert_eq!(eval_int("3 < 5"), 1);
    assert_eq!(eval_int("5 > 3"), 1);
    assert_eq!(eval_int("5 <= 3"), 0);
    assert_eq!(eval_int("3 <= 5"), 1);
    assert_eq!(eval_int("5 >= 5"), 1);
    assert_eq!(eval_int("5 <=> 3"), 1);
    assert_eq!(eval_int("3 <=> 5"), -1);
    assert_eq!(eval_int("5 <=> 5"), 0);
}

#[test]
fn string_comparison_ge_le() {
    assert_eq!(eval_int(r#""b" ge "a""#), 1);
    assert_eq!(eval_int(r#""a" le "b""#), 1);
    assert_eq!(eval_int(r#""abc" gt "abb""#), 1);
}

#[test]
fn logical_and_short_circuit() {
    assert_eq!(eval_int("1 && 5"), 5);
    assert_eq!(eval_int("0 && 5"), 0);
}

#[test]
fn logical_or_short_circuit() {
    assert_eq!(eval_int("0 || 7"), 7);
    assert_eq!(eval_int("3 || 7"), 3);
}

#[test]
fn defined_or_operator() {
    assert_eq!(eval_int("undef // 5"), 5);
    assert_eq!(eval_int("0 // 5"), 0);
}

#[test]
fn logical_words_and_or_not() {
    assert_eq!(eval_int("1 and 2"), 2);
    assert_eq!(eval_int("0 or 9"), 9);
    assert_eq!(eval_int("not 0"), 1);
    assert_eq!(eval_int("not 1"), 0);
}

#[test]
fn logical_not_bang() {
    assert_eq!(eval_int("!1"), 0);
    assert_eq!(eval_int("!0"), 1);
}

#[test]
fn bitwise_operators() {
    assert_eq!(eval_int("0x0f & 0x33"), 0x03);
    assert_eq!(eval_int("0x10 | 0x01"), 0x11);
    assert_eq!(eval_int("0b1010 ^ 0b1100"), 0b0110);
    assert_eq!(eval_int("32 >> 3"), 4);
}

#[test]
fn unary_bitwise_not() {
    assert_eq!(eval_int("~0"), -1);
}

#[test]
fn unary_negate() {
    assert_eq!(eval_int("-42"), -42);
    assert_eq!(eval_int("0 - -1"), 1);
}

#[test]
fn compound_assignment() {
    assert_eq!(eval_int("my $x = 10; $x += 3; $x"), 13);
    assert_eq!(eval_int("my $x = 10; $x -= 4; $x"), 6);
    assert_eq!(eval_int("my $x = 2; $x *= 3; $x"), 6);
    assert_eq!(eval_int("my $x = 2; $x **= 3; $x"), 8);
    assert_eq!(eval_int("my $x = 10; $x %= 3; $x"), 1);
    assert_eq!(eval_string(r#"my $s = "a"; $s .= "b"; $s"#), "ab");
}

#[test]
fn pre_post_increment() {
    assert_eq!(eval_int("my $x = 1; ++$x"), 2);
    assert_eq!(eval_int("my $x = 1; $x++"), 1);
    assert_eq!(eval_int("my $x = 1; $x++; $x"), 2);
}

#[test]
fn pre_post_decrement() {
    assert_eq!(eval_int("my $x = 3; --$x"), 2);
    assert_eq!(eval_int("my $x = 3; $x--"), 3);
    assert_eq!(eval_int("my $x = 3; $x--; $x"), 2);
}

#[test]
fn string_zero_is_false() {
    assert_eq!(eval_int(r#""0" ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#""1" ? 1 : 0"#), 1);
}

#[test]
fn str_cmp_operator() {
    assert_eq!(eval_int(r#""a" cmp "b""#), -1);
    assert_eq!(eval_int(r#""b" cmp "a""#), 1);
    assert_eq!(eval_int(r#""x" cmp "x""#), 0);
}

#[test]
fn hex_binary_literals() {
    assert_eq!(eval_int("0xff"), 255);
    assert_eq!(eval_int("0b1010"), 10);
}

#[test]
fn division_yields_float_coerced_to_int() {
    assert_eq!(eval_int("7 / 2"), 3);
}

#[test]
fn ternary() {
    assert_eq!(eval_int("my $x = 5; $x > 3 ? 1 : 0"), 1);
    assert_eq!(eval_int("my $x = 1; $x > 3 ? 1 : 0"), 0);
}
