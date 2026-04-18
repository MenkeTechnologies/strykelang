//! Tests for extended builtins in `src/builtins_extended.rs`.

use crate::run;

#[test]
fn test_number_theory_builtins() {
    assert_eq!(
        run("join(',', prime_factors(12))")
            .expect("run")
            .to_string(),
        "2,2,3"
    );
    assert_eq!(
        run("join(',', divisors(12))").expect("run").to_string(),
        "1,2,3,4,6,12"
    );
    assert_eq!(run("num_divisors(12)").expect("run").to_int(), 6);
    assert_eq!(run("sum_divisors(12)").expect("run").to_int(), 16);
    assert_eq!(run("is_perfect(28)").expect("run").to_int(), 1);
    assert_eq!(run("is_perfect(12)").expect("run").to_int(), 0);
    assert_eq!(run("is_abundant(12)").expect("run").to_int(), 1);
    assert_eq!(run("is_deficient(10)").expect("run").to_int(), 1);
    assert_eq!(run("collatz_length(6)").expect("run").to_int(), 8);
    assert_eq!(run("lucas(5)").expect("run").to_int(), 11);
    assert_eq!(run("tribonacci(5)").expect("run").to_int(), 4);
    assert_eq!(run("nth_prime(5)").expect("run").to_int(), 11);
    assert_eq!(
        run("join(',', primes_up_to(10))").expect("run").to_string(),
        "2,3,5,7"
    );
    assert_eq!(run("next_prime(13)").expect("run").to_int(), 17);
    assert_eq!(run("prev_prime(13)").expect("run").to_int(), 11);
    assert_eq!(run("triangular_number(5)").expect("run").to_int(), 15);
    assert_eq!(run("pentagonal_number(5)").expect("run").to_int(), 35);
}

#[test]
fn test_statistics_builtins() {
    assert_eq!(run("mean(1, 2, 3, 4, 5)").expect("run").to_number(), 3.0);
    assert_eq!(
        run("variance(1, 2, 3, 4, 5)").expect("run").to_number(),
        2.0
    );
    assert_eq!(
        run("stddev(1, 2, 3, 4, 5)").expect("run").to_number(),
        2.0f64.sqrt()
    );
    assert_eq!(run("median(1, 2, 100)").expect("run").to_number(), 2.0);
    assert_eq!(run("mode(1, 2, 2, 3)").expect("run").to_int(), 2);
}

#[test]
fn test_geometry_builtins() {
    assert!(run("area_circle(1)").expect("run").to_number() > 3.14);
    assert_eq!(run("area_triangle(10, 5)").expect("run").to_number(), 25.0);
    assert_eq!(run("area_rectangle(10, 5)").expect("run").to_number(), 50.0);
    assert_eq!(
        run("perimeter_rectangle(10, 5)").expect("run").to_number(),
        30.0
    );
    assert_eq!(
        run("triangle_hypotenuse(3, 4)").expect("run").to_number(),
        5.0
    );
}

#[test]
fn test_financial_builtins() {
    assert_eq!(run("roi(110, 100)").expect("run").to_number(), 0.1);
    assert_eq!(run("markup(100, 120)").expect("run").to_number(), 20.0);
    assert_eq!(run("margin(100, 125)").expect("run").to_number(), 20.0);
    assert_eq!(run("discount(100, 10)").expect("run").to_number(), 90.0);
    assert_eq!(run("tax(100, 5)").expect("run").to_number(), 105.0);
}

#[test]
fn test_encoding_builtins() {
    assert_eq!(
        run("morse_encode('SOS')").expect("run").to_string(),
        "... --- ..."
    );
    assert_eq!(
        run("morse_decode('... --- ...')").expect("run").to_string(),
        "SOS"
    );
    assert_eq!(
        run("nato_phonetic('ABC')").expect("run").to_string(),
        "Alfa Bravo Charlie"
    );
    assert_eq!(run("int_to_roman(42)").expect("run").to_string(), "XLII");
    assert_eq!(run("roman_to_int('XLII')").expect("run").to_int(), 42);
    assert_eq!(
        run("pig_latin('hello world')").expect("run").to_string(),
        "ellohay orldway"
    );
}

#[test]
fn test_string_builtins() {
    assert_eq!(
        run("join(',', ngrams(2, 'abcde'))")
            .expect("run")
            .to_string(),
        "ab,bc,cd,de"
    );
    assert_eq!(
        run("is_anagram('silent', 'listen')").expect("run").to_int(),
        1
    );
    assert_eq!(
        run("is_pangram('The quick brown fox jumps over the lazy dog')")
            .expect("run")
            .to_int(),
        1
    );
    assert_eq!(
        run("mask_string('12345678', 4)").expect("run").to_string(),
        "****5678"
    );
    assert_eq!(
        run("indent_text('  ', \"a\\nb\")")
            .expect("run")
            .to_string(),
        "  a\n  b"
    );
    assert_eq!(
        run("dedent_text('  a\n  b')").expect("run").to_string(),
        "a\nb"
    );
}

#[test]
fn test_color_builtins() {
    assert_eq!(
        run("join(',', hsl_to_rgb(0, 1, 0.5))")
            .expect("run")
            .to_string(),
        "255,0,0"
    );
    assert_eq!(
        run("join(',', color_invert(255, 255, 255))")
            .expect("run")
            .to_string(),
        "0,0,0"
    );
    assert_eq!(
        run("join(',', color_grayscale(255, 255, 255))")
            .expect("run")
            .to_string(),
        "255,255,255"
    );
}

#[test]
fn test_matrix_builtins() {
    let code = r#"
        my $m = [[1, 2], [3, 4]];
        my $t = matrix_transpose($m);
        matrix_sum($t);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 10);

    let code2 = r#"
        my $m = [[1, 2], [3, 4]];
        matrix_max($m);
    "#;
    assert_eq!(run(code2).expect("run").to_int(), 4);
}

#[test]
fn test_misc_builtins() {
    assert_eq!(
        run("base_convert('1010', 2, 10)").expect("run").to_int(),
        10
    );
    assert_eq!(run("base_convert('A', 16, 10)").expect("run").to_int(), 10);
    assert_eq!(run("bearing(0, 0, 0, 1)").expect("run").to_number(), 90.0);
    assert_eq!(
        run("fizzbuzz(15)->[14]").expect("run").to_string(),
        "FizzBuzz"
    );
    assert_eq!(run("fizzbuzz(3)->[2]").expect("run").to_string(), "Fizz");
    assert_eq!(run("fizzbuzz(5)->[4]").expect("run").to_string(), "Buzz");
    assert_eq!(run("fizzbuzz(2)->[1]").expect("run").to_int(), 2);
}

#[test]
fn test_more_number_theory() {
    assert_eq!(run("is_smith(22)").expect("run").to_int(), 1);
    assert_eq!(run("aliquot_sum(12)").expect("run").to_int(), 16);
    assert_eq!(run("prime_pi(10)").expect("run").to_int(), 4);
    assert_eq!(run("bell_number(3)").expect("run").to_int(), 5);
    assert_eq!(run("subfactorial(3)").expect("run").to_int(), 2);
    assert_eq!(
        run("join(',', collatz_sequence(6))")
            .expect("run")
            .to_string(),
        "6,3,10,5,16,8,4,2,1"
    );
}

#[test]
fn test_more_geometry() {
    assert_eq!(
        run("sphere_volume(1)").expect("run").to_number(),
        (4.0 / 3.0) * std::f64::consts::PI
    );
    assert_eq!(
        run("cylinder_volume(1, 1)").expect("run").to_number(),
        std::f64::consts::PI
    );
    assert_eq!(
        run("cone_volume(1, 1)").expect("run").to_number(),
        std::f64::consts::PI / 3.0
    );
    assert_eq!(
        run("point_distance(0, 0, 3, 4)").expect("run").to_number(),
        5.0
    );
}

#[test]
fn test_more_string_processing() {
    assert_eq!(
        run("camel_to_snake('CamelCase')").expect("run").to_string(),
        "camel_case"
    );
    assert_eq!(
        run("snake_to_camel('snake_case')")
            .expect("run")
            .to_string(),
        "snakeCase"
    );
    assert_eq!(
        run("collapse_whitespace('  a   b  ')")
            .expect("run")
            .to_string(),
        " a b "
    );
    assert_eq!(
        run("remove_vowels('hello')").expect("run").to_string(),
        "hll"
    );
    assert_eq!(
        run("remove_consonants('hello')").expect("run").to_string(),
        "eo"
    );
    assert_eq!(
        run("string_distance('kitten', 'sitting')")
            .expect("run")
            .to_int(),
        3
    );
}

#[test]
fn test_jwt_builtins() {
    let code = r#"
        my $payload = { sub => "1234567890", name => "John Doe", admin => 1 };
        my $secret = "secret";
        my $token = jwt_encode($payload, $secret);
        my $decoded = jwt_decode($token, $secret);
        $decoded->{name};
    "#;
    assert_eq!(run(code).expect("run").to_string(), "John Doe");

    let code_unsafe = r#"
        my $payload = { sub => "1234567890" };
        my $secret = "secret";
        my $token = jwt_encode($payload, $secret);
        my $decoded = jwt_decode_unsafe($token);
        $decoded->{sub};
    "#;
    assert_eq!(run(code_unsafe).expect("run").to_string(), "1234567890");
}

#[test]
fn test_more_color_builtins() {
    assert_eq!(
        run("join(',', color_blend(255, 0, 0, 0, 0, 255, 0.5))")
            .expect("run")
            .to_string(),
        "128,0,128"
    );
}

#[test]
fn test_list_util_builtins() {
    // List::Util functions are available globally in perlrs
    assert_eq!(run("sum(1, 2, 3)").expect("run").to_int(), 6);
    assert_eq!(run("product(2, 3, 4)").expect("run").to_int(), 24);
    assert_eq!(run("min(10, 5, 20)").expect("run").to_int(), 5);
    assert_eq!(run("max(10, 5, 20)").expect("run").to_int(), 20);
    assert_eq!(run("all { $_ > 0 } (1, 2, 3)").expect("run").to_int(), 1);
    assert_eq!(run("any { $_ > 2 } (1, 2, 3)").expect("run").to_int(), 1);
    assert_eq!(run("none { $_ > 5 } (1, 2, 3)").expect("run").to_int(), 1);
    assert_eq!(
        run("join(',', uniq(1, 1, 2, 2, 3))")
            .expect("run")
            .to_string(),
        "1,2,3"
    );
    assert_eq!(
        run("join(',', head(1, 2, 3, 4, 2))")
            .expect("run")
            .to_string(),
        "1,2"
    );
    assert_eq!(
        run("join(',', tail(1, 2, 3, 4, 2))")
            .expect("run")
            .to_string(),
        "3,4"
    );
    assert_eq!(
        run("my @p = pairs(a => 1, b => 2); $p[0]->[0] . ':' . $p[0]->[1]")
            .expect("run")
            .to_string(),
        "a:1"
    );
    assert_eq!(
        run("join(',', mesh([1, 2], [3, 4]))")
            .expect("run")
            .to_string(),
        "1,3,2,4"
    );
    assert_eq!(run("sum0()").expect("run").to_int(), 0);
    assert_eq!(
        run("join(',', reductions sub { $_[0] + $_[1] }, (1, 2, 3))")
            .expect("run")
            .to_string(),
        "1,3,6"
    );
}
