//! Behavior-pinning batch BI (2026-05-08): Validation predicates, Text helpers, and Math series.

use crate::common::*;

// ── Validation Predicates ───────────────────────────────────────────────────

#[test]
fn validation_predicates_bi() {
    // is_anagram (case-insensitive, ignores non-alphanumeric)
    assert_eq!(eval_int(r#"is_anagram("Silent", "Listen")"#), 1);
    assert_eq!(eval_int(r#"is_anagram("Debit card", "Bad credit")"#), 1);
    assert_eq!(eval_int(r#"is_anagram("hello", "world")"#), 0);

    // is_pangram (case-insensitive)
    assert_eq!(eval_int(r#"is_pangram("The quick brown fox jumps over the lazy dog")"#), 1);
    assert_eq!(eval_int(r#"is_pangram("Hello world")"#), 0);

    // is_printable
    assert_eq!(eval_int(r#"is_printable("Hello\tWorld\n")"#), 1);
    assert_eq!(eval_int(r#"is_printable("Hello\x00World")"#), 0);

    // is_control
    assert_eq!(eval_int(r#"is_control("\x01\x02\x03")"#), 1);
    assert_eq!(eval_int(r#"is_control("ABC")"#), 0);

    // is_valid_cron (currently checks for 5 fields)
    assert_eq!(eval_int(r#"is_valid_cron("0 0 * * *")"#), 1);
    assert_eq!(eval_int(r#"is_valid_cron("* * * *")"#), 0);

    // is_valid_mime
    assert_eq!(eval_int(r#"is_valid_mime("text/plain")"#), 1);
    assert_eq!(eval_int(r#"is_valid_mime("application/json")"#), 1);
    assert_eq!(eval_int(r#"is_valid_mime("invalid-mime")"#), 0);

    // is_valid_latitude / longitude
    assert_eq!(eval_int(r#"is_valid_latitude(45.0)"#), 1);
    assert_eq!(eval_int(r#"is_valid_latitude(91.0)"#), 0);
    assert_eq!(eval_int(r#"is_valid_longitude(180.0)"#), 1);
    assert_eq!(eval_int(r#"is_valid_longitude(181.0)"#), 0);

    // is_numeric_string
    assert_eq!(eval_int(r#"is_numeric_string("123.45")"#), 1);
    assert_eq!(eval_int(r#"is_numeric_string("abc")"#), 0);

    // is_valid_hex_color
    assert_eq!(eval_int(r##"is_valid_hex_color("#ff0000")"##), 1);
    assert_eq!(eval_int(r#"is_valid_hex_color("red")"#), 0);
}

// ── Text Helpers ─────────────────────────────────────────────────────────────

#[test]
fn text_helpers_bi() {
    // degrees_to_compass
    assert_eq!(eval_string("degrees_to_compass(0)"), "N");
    assert_eq!(eval_string("degrees_to_compass(45)"), "NE");
    assert_eq!(eval_string("degrees_to_compass(90)"), "E");
    assert_eq!(eval_string("degrees_to_compass(180)"), "S");
    assert_eq!(eval_string("degrees_to_compass(270)"), "W");

    // byte_size
    assert_eq!(eval_int(r#"byte_size("hello")"#), 5);
    assert_eq!(eval_int(r#"byte_size("🚀")"#), 4); // UTF-8 rocket is 4 bytes

    // to_string_val
    assert_eq!(eval_string("to_string_val(42)"), "42");
    assert_eq!(eval_string(r#"to_string_val("hi")"#), "hi");
}

// ── Math Series ──────────────────────────────────────────────────────────────

#[test]
fn math_series_bi() {
    // quadratic_roots(a, b, c) -> solves ax^2 + bx + c = 0
    // x^2 - 5x + 6 = 0 -> roots 3, 2
    let res = eval(r#"quadratic_roots(1, -5, 6)"#).as_array_vec().unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].to_int(), 3);
    assert_eq!(res[1].to_int(), 2);

    // quadratic_discriminant(a, b, c) -> b^2 - 4ac
    // 1, -5, 6 -> (-5)^2 - 4*1*6 = 25 - 24 = 1
    assert_eq!(eval_int("quadratic_discriminant(1, -5, 6)"), 1);

    // arithmetic_series(start, step, n) -> sum of n terms
    // 1, 10, 10 -> 1 + 11 + 21 + 31 + 41 + 51 + 61 + 71 + 81 + 91 = 460
    assert_eq!(eval_int("arithmetic_series(1, 10, 10)"), 460);

    // geometric_series(start, ratio, n) -> sum of n terms
    // 1, 2, 10 -> 1 + 2 + 4 + ... + 512 = 1023
    assert_eq!(eval_int("geometric_series(1, 2, 10)"), 1023);
}

// ── Complex Frequencies & Indexing ──────────────────────────────────────────

#[test]
fn complex_ops_bi() {
    // frequencies on mixed types
    let code = r#"
        my $f = frequencies(1, "1", 2, [1, 2]);
        # [1, 2] is flattened, so we have (1, "1", 2, 1, 2)
        # "1" appears 3 times.
        $f->{"1"}
    "#;
    assert_eq!(eval_int(code), 3);

    // with_index on empty list
    assert_eq!(eval_int("len(with_index())"), 0);

    // interleave with different types
    let code_il = r#"
        my @r = interleave([1, 2], ["a", "b"], [{x=>1}, {y=>2}]);
        len(@r)
    "#;
    assert_eq!(eval_int(code_il), 6);
}
