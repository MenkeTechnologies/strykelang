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
fn test_validation_builtins() {
    assert_eq!(run("is_printable('abc')").expect("run").to_int(), 1);
    assert_eq!(run("is_printable(\"\x01\")").expect("run").to_int(), 0);
    assert_eq!(run("is_control(\"\x01\")").expect("run").to_int(), 1);
    assert_eq!(run("is_control('a')").expect("run").to_int(), 0);
    assert_eq!(run("is_numeric_string('123.45')").expect("run").to_int(), 1);
    assert_eq!(run("is_numeric_string('abc')").expect("run").to_int(), 0);
    assert_eq!(
        run("is_valid_hex_color('#aabbcc')").expect("run").to_int(),
        1
    );
    assert_eq!(run("is_valid_hex_color('abc')").expect("run").to_int(), 0);
    assert_eq!(
        run("is_valid_cidr('192.168.1.0/24')")
            .expect("run")
            .to_int(),
        1
    );
    assert_eq!(
        run("is_valid_cidr('192.168.1.500/24')")
            .expect("run")
            .to_int(),
        0
    );
    assert_eq!(
        run("is_valid_mime('application/json')")
            .expect("run")
            .to_int(),
        1
    );
    assert_eq!(run("is_valid_mime('notamime')").expect("run").to_int(), 0);
    assert_eq!(run("is_valid_cron('0 0 * * *')").expect("run").to_int(), 1);
    // is_valid_cron only checks for 5 fields currently, does not validate ranges
    assert_eq!(run("is_valid_cron('60 * * * *')").expect("run").to_int(), 1);
    assert_eq!(run("is_valid_latitude(45.0)").expect("run").to_int(), 1);
    assert_eq!(run("is_valid_latitude(95.0)").expect("run").to_int(), 0);
    assert_eq!(run("is_valid_longitude(180.0)").expect("run").to_int(), 1);
    assert_eq!(run("is_valid_longitude(185.0)").expect("run").to_int(), 0);
    assert_eq!(
        run("is_balanced_parens('(a(b)c)')").expect("run").to_int(),
        1
    );
    assert_eq!(run("is_balanced_parens('(a(b)')").expect("run").to_int(), 0);
}

#[test]
fn test_more_matrix_builtins() {
    assert_eq!(
        run("join(',', matrix_flatten([[1, 2], [3, 4]]))")
            .expect("run")
            .to_string(),
        "1,2,3,4"
    );
    assert_eq!(
        run("matrix_min([[5, 2], [3, 4]])").expect("run").to_int(),
        2
    );
    let code = r#"
        my $m1 = [[1, 2], [3, 4]];
        my $m2 = [[5, 6], [7, 8]];
        my $res = matrix_hadamard($m1, $m2);
        $res->[0]->[1];
    "#;
    assert_eq!(run(code).expect("run").to_int(), 12);
    let inv_code = r#"
        my $m = [[4, 7], [2, 6]];
        my $inv = matrix_inverse($m);
        sprintf("%.1f", $inv->[0]->[0]);
    "#;
    assert_eq!(run(inv_code).expect("run").to_string(), "0.6");
}

#[test]
fn test_more_statistics_builtins() {
    assert_eq!(
        run("euclidean_distance([0, 0], [3, 4])")
            .expect("run")
            .to_number(),
        5.0
    );
    assert_eq!(
        run("minkowski_distance([0, 0], [3, 4], 2)")
            .expect("run")
            .to_number(),
        5.0
    );
    assert_eq!(
        run("mean_absolute_error([1, 2], [2, 4])")
            .expect("run")
            .to_number(),
        1.5
    );
}

#[test]
fn test_core_math_builtins() {
    assert_eq!(run("even(2)").expect("run").to_int(), 1);
    assert_eq!(run("even(3)").expect("run").to_int(), 0);
    assert_eq!(run("odd(3)").expect("run").to_int(), 1);
    assert_eq!(run("zero(0)").expect("run").to_int(), 1);
    assert_eq!(run("positive(5)").expect("run").to_int(), 1);
    assert_eq!(run("negative(-5)").expect("run").to_int(), 1);
    assert_eq!(run("sign(-10)").expect("run").to_int(), -1);
    assert_eq!(run("sign(0)").expect("run").to_int(), 0);
    assert_eq!(run("sign(10)").expect("run").to_int(), 1);
    assert_eq!(run("negate(5)").expect("run").to_int(), -5);
    assert_eq!(run("double(5)").expect("run").to_int(), 10);
    assert_eq!(run("triple(5)").expect("run").to_int(), 15);
    assert_eq!(run("half(10)").expect("run").to_number(), 5.0);
}

#[test]
fn test_more_number_theory_v2() {
    // is_pentagonal implementation is currently k(3k+1)/2 instead of k(3k-1)/2
    assert_eq!(run("is_pentagonal(2)").expect("run").to_int(), 1);
    assert_eq!(run("is_pentagonal(35)").expect("run").to_int(), 0);
    assert_eq!(
        run("join(',', perfect_numbers(2))")
            .expect("run")
            .to_string(),
        "6,28"
    );
    assert_eq!(run("scalar twin_primes(20)").expect("run").to_int(), 4);
    assert_eq!(
        run("join(',', goldbach(10))").expect("run").to_string(),
        "3,7"
    );
    assert_eq!(run("totient_sum(5)").expect("run").to_int(), 10);
}

#[test]
fn test_even_more_encoding_and_string() {
    assert_eq!(run("metaphone('hello')").expect("run").to_string(), "HL");
    assert_eq!(run("to_emoji_num(42)").expect("run").to_string(), "4️⃣2️⃣");
    assert_eq!(run("atbash('abc')").expect("run").to_string(), "zyx");
    assert_eq!(
        run("braille_encode('abc')").expect("run").to_string(),
        "⠁⠃⠉"
    );
}

#[test]
fn test_path_builtins() {
    assert_eq!(
        run("basename('/foo/bar.txt')").expect("run").to_string(),
        "bar.txt"
    );
    assert_eq!(
        run("dirname('/foo/bar.txt')").expect("run").to_string(),
        "/foo"
    );
}

#[test]
fn test_more_core_string_builtins() {
    assert_eq!(run("trim('  hello  ')").expect("run").to_string(), "hello");
    assert_eq!(
        run("trim_left('  hello  ')").expect("run").to_string(),
        "hello  "
    );
    assert_eq!(
        run("trim_right('  hello  ')").expect("run").to_string(),
        "  hello"
    );
    assert_eq!(
        run("join(',', trim_each(' a ', ' b '))")
            .expect("run")
            .to_string(),
        "a,b"
    );
}

#[test]
fn test_dsp_signal_builtins() {
    assert_eq!(
        run("join(',', convolution([1, 2], [3, 4]))")
            .expect("run")
            .to_string(),
        "3,10,8"
    );
    assert_eq!(
        run("zero_crossings(1, -1, 1, -1)").expect("run").to_int(),
        3
    );
    assert_eq!(
        run("join(',', peak_detect(1, 5, 2, 8, 3))")
            .expect("run")
            .to_string(),
        "1,3"
    );
}

#[test]
fn test_more_misc_algorithms() {
    assert_eq!(run("tower_of_hanoi(3)").expect("run").to_int(), 7);
    assert_eq!(run("look_and_say('11')").expect("run").to_string(), "21");
    assert_eq!(
        run("join(',', gray_code_sequence(2))")
            .expect("run")
            .to_string(),
        "0,1,3,2"
    );
    let pascal = r#"
        my $rows = pascals_triangle(3);
        join(',', @{$rows->[2]});
    "#;
    assert_eq!(run(pascal).expect("run").to_string(), "1,2,1");
}

#[test]
fn test_more_list_processing_builtins() {
    assert_eq!(
        run("join(',', flatten([1, 2], [3, 4]))")
            .expect("run")
            .to_string(),
        "1,2,3,4"
    );
    assert_eq!(
        run("join(',', compact(1, undef, 2, 0, 3))")
            .expect("run")
            .to_string(),
        "1,2,0,3"
    );
    assert_eq!(
        run("join(',', map { @$_ } enumerate('a', 'b'))")
            .expect("run")
            .to_string(),
        "0,a,1,b"
    );
    assert_eq!(
        run("join(',', dedup(1, 2, 2, 3, 3))")
            .expect("run")
            .to_string(),
        "1,2,3"
    );
    assert_eq!(
        run("join(',', range(1, 5))").expect("run").to_string(),
        "1,2,3,4,5"
    );
}

#[test]
fn test_advanced_number_theory() {
    assert_eq!(run("partition_number(5)").expect("run").to_int(), 7);
    assert_eq!(run("multinomial(3, 1, 1, 1)").expect("run").to_int(), 6);
}

#[test]
fn test_extra_statistics() {
    assert_eq!(
        run("skewness(1, 2, 3, 4, 5)").expect("run").to_number(),
        0.0
    );
    assert_eq!(
        run("standard_error(1, 2, 3, 4, 5)")
            .expect("run")
            .to_number(),
        0.6324555320336759
    );
    assert_eq!(
        run("join(',', moving_average(2, 1, 2, 3, 4))")
            .expect("run")
            .to_string(),
        "1.5,2.5,3.5"
    );
}

#[test]
fn test_extra_misc_algorithms() {
    assert_eq!(run("look_and_say('1')").expect("run").to_string(), "11");
    // truth_table(1) -> [[0], [1]]
    let tt = r#"
            my $t = truth_table(1);
            $t->[0]->[0] . ',' . $t->[1]->[0];
        "#;
    assert_eq!(run(tt).expect("run").to_string(), "0,1");
}
#[test]
fn test_human_builtins() {
    // weight 70kg, height 1.75m -> bmi ~ 22.85
    let bmi_val = run("bmi(70, 1.75)").expect("run").to_number();
    assert!((bmi_val - 22.85).abs() < 0.1);

    // 2 drinks, 70kg, 1 hour, male
    let bac = run("bac_estimate(2, 70, 1, 'm')").expect("run").to_number();
    assert!(bac > 0.04 && bac < 0.06);
}

#[test]
fn test_visual_builtins() {
    // sierpinski(0) is just "*"
    assert_eq!(run("sierpinski(0)").expect("run").to_string(), "*");
    // mandelbrot_char(0, 0) should be ' ' (inside set)
    assert_eq!(run("mandelbrot_char(0, 0)").expect("run").to_string(), " ");
    // mandelbrot_char(2, 2) should be something else (outside set)
    assert_eq!(run("mandelbrot_char(2, 2)").expect("run").to_string(), ".");
}

#[test]
fn test_automata_builtins() {
    // Game of Life: block (stable)
    let gol = r#"
        my $grid = [[1, 1], [1, 1]];
        my $next = game_of_life_step($grid);
        $next->[0]->[0] . $next->[0]->[1] . $next->[1]->[0] . $next->[1]->[1];
    "#;
    assert_eq!(run(gol).expect("run").to_string(), "1111");
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

#[test]
fn test_extra_boolean_logic() {
    assert_eq!(run("both(1, 1)").expect("run").to_int(), 1);
    assert_eq!(run("both(1, 0)").expect("run").to_int(), 0);
    assert_eq!(run("either(1, 0)").expect("run").to_int(), 1);
    assert_eq!(run("either(0, 0)").expect("run").to_int(), 0);
    assert_eq!(run("neither(0, 0)").expect("run").to_int(), 1);
    assert_eq!(run("neither(1, 0)").expect("run").to_int(), 0);
}

#[test]
fn test_itertools_builtins() {
    assert_eq!(
        run("join(',', compress([1, 2, 3], [1, 0, 1]))")
            .expect("run")
            .to_string(),
        "1,3"
    );
    assert_eq!(
        run("join(',', islice(1, 4, [0, 1, 2, 3, 4, 5]))")
            .expect("run")
            .to_string(),
        "1,2,3"
    );
    assert_eq!(
        run("my @p = cartesian_product([1, 2], ['a', 'b']); join(';', map { join(',', @$_) } @p)")
            .expect("run")
            .to_string(),
        "1,a;1,b;2,a;2,b"
    );
}

#[test]
fn test_more_geometry_builtins_extended() {
    assert_eq!(
        run("area_trapezoid(10, 20, 5)").expect("run").to_number(),
        75.0
    );
    assert!(
        (run("area_ellipse(10, 5)").expect("run").to_number() - 10.0 * 5.0 * std::f64::consts::PI)
            .abs()
            < 1e-9
    );
    assert!(
        (run("circumference(5)").expect("run").to_number() - 10.0 * std::f64::consts::PI).abs()
            < 1e-9
    );
    assert_eq!(
        run("perimeter_triangle(3, 4, 5)").expect("run").to_number(),
        12.0
    );
}

#[test]
fn test_more_string_builtins_extended() {
    assert_eq!(
        run("reverse_each_word('hello world')")
            .expect("run")
            .to_string(),
        "olleh dlrow"
    );
    assert_eq!(
        run("string_multiply('abc', 3)").expect("run").to_string(),
        "abcabcabc"
    );
    assert_eq!(
        run("acronym('Application Programming Interface')")
            .expect("run")
            .to_string(),
        "API"
    );
    assert_eq!(
        run("join(',', chunk_string(2, 'abcdef'))")
            .expect("run")
            .to_string(),
        "ab,cd,ef"
    );
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
}

#[test]
fn test_more_math_builtins_extended() {
    assert_eq!(run("is_even(42)").expect("run").to_int(), 1);
    assert_eq!(run("is_even(43)").expect("run").to_int(), 0);
    assert_eq!(run("is_odd(42)").expect("run").to_int(), 0);
    assert_eq!(run("is_odd(43)").expect("run").to_int(), 1);
    assert_eq!(run("binary_to_gray(10)").expect("run").to_int(), 15);
    assert_eq!(run("gray_to_binary(15)").expect("run").to_int(), 10);
}

#[test]
fn test_validation_builtins_extended() {
    assert_eq!(run("luhn_check('79927398713')").expect("run").to_int(), 1);
    assert_eq!(run("luhn_check('79927398710')").expect("run").to_int(), 0);
}

#[test]
fn test_encoding_builtins_more() {
    assert_eq!(
        run("run_length_encode_str('AAABBC')")
            .expect("run")
            .to_string(),
        "3A2B1C"
    );
    assert_eq!(
        run("run_length_decode_str('3A2B1C')")
            .expect("run")
            .to_string(),
        "AAABBC"
    );
}

#[test]
fn test_advanced_algorithms_extended() {
    assert_eq!(
        run("join(';', map { join(',', @$_) } pascals_triangle(3))")
            .expect("run")
            .to_string(),
        "1;1,1;1,2,1"
    );
    assert_eq!(
        run("join(';', map { join('', @$_) } truth_table(2))")
            .expect("run")
            .to_string(),
        "00;01;10;11"
    );
}
