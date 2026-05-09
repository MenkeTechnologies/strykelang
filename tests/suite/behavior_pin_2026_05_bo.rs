//! Behavior-pinning batch BO (2026-05-08): ~20 test fns covering distances/ND geometry, aggregates,
//! primes & multinomials, hashing & encoding, string transforms & padding, deep_merge & types,
//! take_n/drop/swap/rotate/zip, polynomials & det, bit ops, JSON, Roman numerals, iota/tally/window.

use crate::common::*;

#[test]
fn distances_similarity_bo() {
    assert_eq!(eval_int("euclidean_distance([0, 0], [3, 4])"), 5);
    assert_eq!(eval_int("manhattan_distance([1, 2], [4, 6])"), 7);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", cosine_similarity([1, 0], [1, 1]))"#),
        "0.707107"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10f", centroid([1, 2, 3]))"#),
        "2.0000000000"
    );
}

#[test]
fn polygon_and_nd_geometry_bo() {
    assert_eq!(
        eval_string(
            r#"my @c = polygon_centroid([[0, 0], [4, 0], [4, 3], [0, 3]]);
            join(",", map { sprintf("%.4f", $_) } @c)"#
        ),
        "2.0000,1.5000"
    );
    assert_eq!(
        eval_string(
            r#"my @c = centroid_nd([[1, 0], [3, 4], [5, 8]]);
            join(",", map { sprintf("%.2f", $_) } @c)"#
        ),
        "3.00,4.00"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10f", euclidean_distance_nd([1, 2, 3], [1, 5, 11]))"#
        ),
        "8.5440037453"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.6f", point_to_plane_distance(0, 0, 0, 1, 1, 1, sqrt(3)))"#
        ),
        "0.000000"
    );
}

#[test]
fn aggregates_spread_bo() {
    assert_eq!(eval_int("sum(1, 2, 3, 4)"), 10);
    assert_eq!(eval_int("product(2, 3, 4)"), 24);
    assert_eq!(eval_int("mean(2, 4, 6)"), 4);
    assert_eq!(
        eval_string(r#"sprintf("%.15f", variance(1, 2, 3, 4, 5))"#),
        "2.000000000000000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.14f", stddev(2, 4, 6))"#),
        "1.63299316185545"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.14f", harmonic_mean(2, 4, 8))"#),
        "3.42857142857143"
    );
    assert_eq!(eval_int("geometric_mean(1, 9)"), 3);
    assert_eq!(
        eval_string(r#"sprintf("%.14f", rms(3, 4))"#),
        "3.53553390593274"
    );
    assert_eq!(eval_int("sum(1:100)"), 5050);
}

#[test]
fn primes_factorials_combin_bo() {
    assert_eq!(
        eval_string(r#"join(",", primes_up_to(20))"#),
        "2,3,5,7,11,13,17,19"
    );
    assert_eq!(eval_int("next_prime(20)"), 23);
    assert_eq!(eval_int("prev_prime(20)"), 19);
    assert_eq!(eval_int("factorial(6)"), 720);
    assert_eq!(eval_int("binomial(5, 2)"), 10);
    assert_eq!(eval_int("multinomial(10, 2, 3, 5)"), 2520);
    assert_eq!(eval_int("lcm(lcm(4, 6), 10)"), 60);
    assert_eq!(eval_int("gcd(gcd(48, 72), 96)"), 24);
}

#[test]
fn hashing_encoding_bo() {
    assert_eq!(
        eval_string(r#"substring(md5("hello"), 0, 8)"#),
        "5d41402a"
    );
    assert_eq!(
        eval_string(r#"substring(sha256("abc"), 0, 16)"#),
        "ba7816bf8f01cfea"
    );
    assert_eq!(eval_string(r#"base64_encode("hi")"#), "aGk=");
    assert_eq!(eval_string(r#"base64_decode("aGk=")"#), "hi");
    assert_eq!(eval_int(r#"crc32("hello")"#), 907_060_870);
    assert_eq!(eval_int(r#"adler32("hello")"#), 103_547_413);
    assert_eq!(eval_string(r#"hex_encode("AB")"#), "4142");
    assert_eq!(eval_string(r#"url_encode("a b")"#), "a%20b");
}

#[test]
fn strings_case_padding_bo() {
    assert_eq!(
        eval_string(r#"slugify("Hello World!")"#),
        "hello-world"
    );
    assert_eq!(
        eval_string(r#"title_case("hello world")"#),
        "Hello World"
    );
    assert_eq!(eval_string(r#"camel_case("foo_bar")"#), "fooBar");
    assert_eq!(eval_string(r#"snake_case("FooBar")"#), "foo_bar");

    assert_eq!(eval_string(r#"pad_left("7", 5, "0")"#), "00007");
    assert_eq!(eval_string(r#"pad_right("7", 5, "0")"#), "70000");
    assert_eq!(eval_string(r#"repeat_string("ab", 3)"#), "ababab");
    assert_eq!(
        eval_string(r#"truncate_at("hello world", 8)"#),
        concat!("hello w", '\u{2026}')
    );
}

#[test]
fn merge_predicate_types_bo() {
    assert_eq!(
        eval_string(r#"stringify(deep_merge({ a => 1 }, { b => 2 }))"#),
        "+{a => 1, b => 2}"
    );
    assert_eq!(eval_int("defined(undef)"), 0);
    assert_eq!(eval_int(r#"is_numeric("3.5")"#), 1);
    assert_eq!(eval_int(r#"is_numeric_string("3.5")"#), 1);
    assert_eq!(eval_string(r#"typeof(42)"#), "integer");
    assert_eq!(eval_string(r#"typeof({ a => 1 })"#), "hashref");
}

#[test]
fn list_take_drop_rotate_bo() {
    assert_eq!(
        eval_string(r#"join(",", take_n(3, 10, 20, 30, 40))"#),
        "10,20,30"
    );
    assert_eq!(
        eval_string(r#"join(",", drop(10, 20, 30, 40, 2))"#),
        "30,40"
    );
    assert_eq!(
        eval_string(r#"join(",", rotate(-1, 1, 2, 3, 4))"#),
        "4,1,2,3"
    );
    assert_eq!(
        eval_string(r#"join(",", swap_pairs(1, 2, 3, 4))"#),
        "2,1,4,3"
    );
    assert_eq!(
        eval_string(r#"join(",", uniq(1, 2, 2, 3, 1))"#),
        "1,2,3"
    );
}

#[test]
fn polynomials_det_bo() {
    assert_eq!(eval_int("horner([1, 2, 3], 10)"), 321);
    assert_eq!(eval_int("polyval([1, 0, -1], 3)"), -8);
    assert_eq!(
        eval_string(
            r#"sprintf("%.0f", det([[1, 2, 3], [0, 1, 4], [5, 6, 0]]))"#
        ),
        "1"
    );
}

#[test]
fn bits_bo() {
    assert_eq!(eval_int("bit_length(255)"), 8);
    assert_eq!(eval_int("bit_reverse_32(1)"), 2_147_483_648);
}

#[test]
fn json_scalar_roundtrip_bo() {
    assert_eq!(
        eval_int(
            r##"my $h = json_decode('{"answer":41}'); $h->{"answer"}"##
        ),
        41
    );
    assert_eq!(
        eval_string(
            r##"substring(json_encode({ ping => 'pong' }), 0, 20)"##
        ),
        r##"{"ping":"pong"}"##
    );
}

#[test]
fn zip_and_transpose_lists_bo() {
    assert_eq!(
        eval_string(
            r#"my @z = zip([1, 2], [10, 20]);
            join(":", map { join(",", @$_) } @z)"#
        ),
        "1,10:2,20"
    );
}

#[test]
fn bit_logic_more_bo() {
    assert_eq!(eval_int("bit_xor(255, 15)"), 240);
    assert_eq!(eval_int("bit_clear(255, 0)"), 254);
    assert_eq!(eval_int("bit_set(0, 3)"), 8);
}

#[test]
fn roman_trunc_numeric_bo() {
    assert_eq!(eval_string(r#"int_to_roman(9)"#), "IX");
    assert_eq!(eval_int(r#"roman_to_int("IX")"#), 9);
    assert_eq!(eval_int(r#"trunc(-3.7)"#), -3);
    assert_eq!(eval_int(r#"trunc(9.99)"#), 9);
}

#[test]
fn round_divmod_minmax_bo() {
    assert_eq!(eval_int("round(2.5)"), 3);
    assert_eq!(eval_int("ceil_div(10, 3)"), 4);
    assert_eq!(eval_string(r#"join(",", divmod(10, 3))"#), "3,1");
    assert_eq!(
        eval_string(r#"longest("a", "bbb", "cc")"#),
        "bbb"
    );
}

#[test]
fn min_max_split_join_bo() {
    assert_eq!(eval_int("min(3, 1, 4, 1, 5)"), 1);
    assert_eq!(eval_int("max(3, 1, 4, 1, 5)"), 5);
    assert_eq!(
        eval_string(r#"join("-", split(",", "a,b,c"))"#),
        "a-b-c"
    );
}

#[test]
fn numeric_predicate_bo() {
    assert_eq!(eval_int("even(4)"), 1);
    assert_eq!(eval_int("odd(7)"), 1);
    assert_eq!(eval_int("between(5, 1, 10)"), 1);
    assert_eq!(eval_int("in_range(7, 7, 12)"), 1);
    assert_eq!(eval_int("clamp(5, 10, 100)"), 10);
    assert_eq!(eval_int("sign(42)"), 1);
    assert_eq!(eval_int("abs(-9)"), 9);
}

#[test]
fn float_classifier_bo() {
    assert_eq!(eval_int("is_finite(3.5)"), 1);
    assert_eq!(eval_int("is_nan(0)"), 0);
}

#[test]
fn tally_replicate_iota_window_bo() {
    assert_eq!(
        eval_string(r#"stringify(tally(1, 2, 2, 3))"#),
        "+{1 => 1, 2 => 2, 3 => 1}"
    );
    assert_eq!(
        eval_string(r#"join(",", repeat_elem(9, 3))"#),
        "9,9,9"
    );
    assert_eq!(eval_string(r#"join(",", iota(5))"#), "0,1,2,3,4");
    assert_eq!(
        eval_string(r#"stringify(windowed_circular(2, 1, 2, 3))"#),
        "([1, 2], [2, 3], [3, 1])"
    );
}

#[test]
fn string_chars_length_bo() {
    assert_eq!(eval_int(r#"length("hello")"#), 5);
    assert_eq!(eval_int(r#"len(chars("x"))"#), 1);
}
