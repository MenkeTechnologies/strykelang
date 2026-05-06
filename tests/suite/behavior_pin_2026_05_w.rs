//! Behavior-pinning batch W (2026-05-05): Matrix, Unit Conversions, String Extensions, System.

use crate::common::*;

// ── Matrix Operations ────────────────────────────────────────────────────────

#[test]
fn matrix_transpose_2x2() {
    let code = r#"
        my $t = matrix_transpose([[1, 2], [3, 4]]);
        # $t is an arrayref of arrayrefs: [[1, 3], [2, 4]]
        join(":", map { join(",", @$_) } @$t)
    "#;
    assert_eq!(eval_string(code), "1,3:2,4");
}

#[test]
fn matrix_hadamard_product() {
    let code = r#"
        my $m1 = [[1, 2], [3, 4]];
        my $m2 = [[5, 6], [7, 8]];
        my @h = matrix_hadamard($m1, $m2);
        # matrix_hadamard returns list of arrayrefs
        join(":", map { join(",", @$_) } @h)
    "#;
    assert_eq!(eval_string(code), "5,12:21,32");
}

#[test]
fn matrix_identity_and_power() {
    let code = r#"
        my @i = matrix_identity(2);
        # matrix_identity returns list of arrayrefs: ([1, 0], [0, 1])
        my $flat = join(",", matrix_flatten(\@i));
        
        # Capture as array to avoid taking only the first row in scalar context
        my @p = matrix_power(\@i, 3);
        my $flat_p = join(",", matrix_flatten(\@p));
        "$flat:$flat_p"
    "#;
    assert_eq!(eval_string(code), "1,0,0,1:1,0,0,1");
}

#[test]
fn matrix_map_scaling() {
    let code = r#"
        my $m = [[1, 2], [3, 4]];
        my @res = matrix_map(sub { $_[0] * 10 }, $m);
        join(":", map { join(",", @$_) } @res)
    "#;
    assert_eq!(eval_string(code), "10,20:30,40");
}

// ── Unit Conversions ─────────────────────────────────────────────────────────

#[test]
fn unit_conversions_temp() {
    assert_eq!(eval_int("c_to_f(0)"), 32);
    assert_eq!(eval_int("f_to_c(32)"), 0);
    assert_eq!(eval_int("c_to_k(0)"), 273); // approx
}

#[test]
fn unit_conversions_distance() {
    // 1 mile ~ 1.609 km
    assert!(eval("miles_to_km(1)").to_number() > 1.6);
    assert!(eval("km_to_miles(1.60934)").to_number() > 0.99);
}

#[test]
fn unit_conversions_digital() {
    assert_eq!(eval_int("bytes_to_kb(1024)"), 1);
    assert_eq!(eval_int("mb_to_bytes(1)"), 1048576);
}

// ── String Extensions ────────────────────────────────────────────────────────

#[test]
fn string_ngrams_and_cases() {
    assert_eq!(eval_string(r#"join(",", ngrams(2, "abc"))"#), "ab,bc");
    assert_eq!(eval_string(r#"pascal_case("hello_world")"#), "HelloWorld");
    assert_eq!(eval_string(r#"constant_case("hello_world")"#), "HELLO_WORLD");
}

#[test]
fn string_predicates() {
    assert_eq!(eval_int(r#"is_palindrome("racecar")"#), 1);
    assert_eq!(eval_int(r#"is_palindrome("hello")"#), 0);
    assert_eq!(eval_int(r#"hamming_distance("karolin", "kathrin")"#), 3);
}

// ── System Introspection ─────────────────────────────────────────────────────

#[test]
fn system_info_smoke() {
    // These should return non-empty strings or positive numbers
    assert!(!eval_string("os_name()").is_empty());
    assert!(eval_int("num_cpus()") > 0);
    assert!(eval_int("pid()") > 0);
}

#[test]
fn system_git_smoke() {
    let code = r#"
        my @log = git_log(1);
        if (len(@log) > 0) {
            my $msg = $log[0]->{message};
            len($msg) > 0 ? 1 : 0
        } else {
            1 # Skip if not in a git repo during test (though it usually is)
        }
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Formatting Helpers ───────────────────────────────────────────────────────

#[test]
fn format_helpers_smoke() {
    assert_eq!(eval_string("human_bytes(1024)"), "1.00 KB");
    assert!(eval_string("human_duration(65)").contains("1m"));
    assert_eq!(eval_string("format_percent(50, 2)"), "50.00%");
}
