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
}
