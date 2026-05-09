//! Behavior-pinning batch BV (2026-05-08): string similarity (`similarity`, `jaro_similarity`,
//! `jaro_winkler`, `jaccard_index`, `dice_coefficient`, `jaccard_similarity`), vector distances
//! (`cosine_similarity`, `euclidean_distance`, `manhattan_distance`, `haversine_distance`), text
//! features (`ngrams`, `is_pangram`, `is_anagram`, `is_palindrome`, `hamming_distance`, phonetic
//! `soundex`/`metaphone`, case helpers `title_case`/`snake_case`/`camel_case`), CSV/encoding (`fold_left`,
//! `from_csv_line`, `base64_*`, `crc32`, `md5`, `sha256`), stats (`matrix_trace`, skew/kurtosis,
//! covariance/correlation, `percentile`, `iqr`, `mad`, `entropy`, `cross_entropy`, `binomial`),
//! utilities (`word_count`, `line_count`, `rot13`, `zipmap`, `reverse` chars idiom, Perl `&`/`ord`/`chr`).

use crate::common::*;

#[test]
fn similarity_ratio_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", similarity("abcd", "abXd"))"#),
        "0.7500"
    );
}

#[test]
fn jaro_similarity_small_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", jaro_similarity("a", "ab"))"#),
        "0.8333"
    );
}

#[test]
fn jaro_winkler_classic_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", jaro_winkler("dwayne", "duane"))"#),
        "0.8400"
    );
}

#[test]
fn jaccard_index_arrays_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", jaccard_index([1, 2, 3], [2, 3, 4]))"#),
        "0.5000"
    );
}

#[test]
fn dice_coefficient_arrays_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", dice_coefficient([1, 2, 3], [2, 3, 4]))"#),
        "0.6667"
    );
}

#[test]
fn jaccard_similarity_strings_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", jaccard_similarity("abcd", "cdef"))"#),
        "0.0000"
    );
}

#[test]
fn cosine_parallel_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", cosine_similarity([1, 0], [1, 0]))"#),
        "1.0000"
    );
}

#[test]
fn euclidean_right_triangle_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", euclidean_distance([0, 0], [3, 4]))"#),
        "5.0000"
    );
}

#[test]
fn manhattan_axis_steps_bv() {
    assert_eq!(eval_int(r#"int(manhattan_distance([1, 2], [10, 20]))"#), 27);
}

#[test]
fn haversine_same_point_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", haversine_distance(0, 0, 0, 0))"#),
        "0.000000"
    );
}

#[test]
fn ngrams_order_two_bv() {
    assert_eq!(
        eval_string(r#"stringify(ngrams(2, "abcde"))"#),
        r##"("ab", "bc", "cd", "de")"##
    );
}

#[test]
fn is_pangram_english_bv() {
    assert_eq!(
        eval_int(r#"is_pangram("the quick brown fox jumps over the lazy dog")"#),
        1
    );
}

#[test]
fn is_anagram_listen_bv() {
    assert_eq!(eval_int(r#"is_anagram("listen", "silent")"#), 1);
}

#[test]
fn is_palindrome_racecar_bv() {
    assert_eq!(eval_int(r#"is_palindrome("racecar")"#), 1);
}

#[test]
fn fold_left_sum_bv() {
    assert_eq!(eval_int(r#"int(fold_left(1, 2, 3, 4))"#), 10);
}

#[test]
fn from_csv_line_quoted_bv() {
    assert_eq!(
        eval_string(r#"join("|", from_csv_line("a,\"b,c\",d"))"#),
        "a|b,c|d"
    );
}

#[test]
fn matrix_trace_two_bv() {
    assert_eq!(eval_int(r#"int(matrix_trace([[1, 9], [3, 4]]))"#), 5);
}

#[test]
fn skewness_kurtosis_uniformish_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", skewness(1, 2, 3, 4, 5))"#),
        "0.0000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.4f", kurtosis(1, 2, 3, 4, 5))"#),
        "-1.3000"
    );
}

#[test]
fn covariance_correlation_linear_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", covariance([1, 2, 3], [6, 7, 8]))"#),
        "1.0000"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.6f", correlation([1, 2, 3], [6, 7, 8]))"#),
        "1.000000"
    );
}

#[test]
fn percentile_builtin_median_bv() {
    assert_eq!(
        eval_int(r#"int(percentile(50, 1, 2, 3, 4, 5, 6, 7, 8, 9))"#),
        5
    );
}

#[test]
fn interquartile_range_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", iqr(1, 2, 3, 4, 100))"#),
        "2.0000"
    );
}

#[test]
fn median_absolute_deviation_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", mad(1, 2, 9, 10))"#),
        "7.0000"
    );
}

#[test]
fn entropy_uniform_four_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", entropy(0.25, 0.25, 0.25, 0.25))"#),
        "1.386294"
    );
}

#[test]
fn cross_entropy_uniform_match_bv() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", cross_entropy([0.5, 0.5], [0.5, 0.5]))"#),
        "0.693147"
    );
}

#[test]
fn binomial_coefficient_bv() {
    assert_eq!(eval_int(r#"binomial(5, 2)"#), 10);
}

#[test]
fn rot13_hello_bv() {
    assert_eq!(eval_string(r#"rot13("hello")"#), "uryyb");
}

#[test]
fn base64_roundtrip_hi_bv() {
    assert_eq!(eval_string(r#"base64_encode("hi")"#), "aGk=");
    assert_eq!(eval_string(r#"base64_decode("aGk=")"#), "hi");
}

#[test]
fn crc32_hello_bv() {
    assert_eq!(eval_int(r#"crc32("hello")"#), 907_060_870);
}

#[test]
fn md5_abc_digest_bv() {
    assert_eq!(
        eval_string(r#"md5("abc")"#),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}

#[test]
fn sha256_empty_digest_bv() {
    assert_eq!(
        eval_string(r#"sha256("")"#),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn reverse_chars_join_bv() {
    assert_eq!(eval_string(r#"join("", reverse(split(//, "abc")))"#), "cba");
}

#[test]
fn zipmap_key_value_bv() {
    assert_eq!(
        eval_string(r#"stringify(zipmap([7, 8], [90, 91]))"#),
        "+{7 => 90, 8 => 91}"
    );
}

#[test]
fn soundex_smith_bv() {
    assert_eq!(eval_string(r#"soundex("Smith")"#), "S530");
}

#[test]
fn metaphone_smith_bv() {
    assert_eq!(eval_string(r#"metaphone("Smith")"#), "SM0");
}

#[test]
fn hamming_distance_equal_len_bv() {
    assert_eq!(eval_int(r#"hamming_distance("0000", "1111")"#), 4);
}

#[test]
fn word_and_line_counts_bv() {
    assert_eq!(eval_int(r#"word_count("  one two\tthree\n")"#), 3);
    assert_eq!(eval_int(r#"line_count("a\nb\nc\n")"#), 3);
}

#[test]
fn title_snake_camel_bv() {
    assert_eq!(eval_string(r#"title_case("the quick")"#), "The Quick");
    assert_eq!(eval_string(r#"snake_case("FooBar")"#), "foo_bar");
    assert_eq!(eval_string(r#"camel_case("foo_bar")"#), "fooBar");
}

#[test]
fn perl_bitwise_and_ord_chr_bv() {
    assert_eq!(eval_int(r#"15 & 10"#), 10);
    assert_eq!(eval_int(r#"ord("A")"#), 65);
    assert_eq!(eval_string(r#"chr(66)"#), "B");
}
