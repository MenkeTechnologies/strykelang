//! Behavior-pinning batch CN (2026-05-09): regex helpers, combinatorics / number theory (`gamma`,
//! primes, `mobius`), running aggregates, `pack`/`unpack`, serializers (`to_toml` / `yaml_encode`),
//! string metrics (`levenshtein`, `jaccard`, phonetics), `ngrams` / `bigrams`, matrix helpers
//! (`transpose` variadic vs nested — **BUG-159**), stats corner cases, `parse_int` hex (**BUG-158**),
//! and **regex builtin argument-order split** (**BUG-160**).

use crate::common::*;

#[test]
fn count_regex_matches_digits_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", count_regex_matches("a1b2c3", "\\d"))"#),
        "3"
    );
}

#[test]
fn split_regex_csv_cn() {
    assert_eq!(
        eval_string(r#"stringify(split_regex(",", "a,b,c"))"#),
        r#"("a", "b", "c")"#
    );
}

#[test]
fn longest_common_substring_sample_cn() {
    assert_eq!(
        eval_string(r#"longest_common_substring("ABCDEF", "GBCDFE")"#),
        "BCD"
    );
}

#[test]
fn lcs_length_sample_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", lcs_length("ABCDEF", "GBCDFE"))"#),
        "4"
    );
}

#[test]
fn gamma_integer_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", gamma(5))"#), "24");
}

#[test]
fn nth_prime_ten_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", nth_prime(10))"#), "29");
}

#[test]
fn mobius_thirty_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", mobius(30))"#), "-1");
}

#[test]
fn running_max_increasing_prefix_cn() {
    assert_eq!(
        eval_string(r#"stringify(running_max([3, 1, 4, 2]))"#),
        "(3, 3, 4, 4)"
    );
}

#[test]
fn running_min_decreasing_prefix_cn() {
    assert_eq!(
        eval_string(r#"stringify(running_min([3, 1, 4, 1, 5]))"#),
        "(3, 1, 1, 1, 1)"
    );
}

#[test]
fn coalesce_skips_undef_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", coalesce(undef, undef, 7))"#),
        "7"
    );
}

#[test]
fn parse_int_hex_with_radix_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", parse_int("ff", 16))"#),
        "255"
    );
}

#[test]
fn parse_int_zero_x_without_radix_is_zero_bug_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", parse_int("0xff"))"#), "0");
}

#[test]
fn unpack_pack_unsigned_short_roundtrip_cn() {
    assert_eq!(
        eval_string(r#"stringify(unpack("S", pack("S", 513)))"#),
        "513"
    );
}

#[test]
fn replace_regex_global_digits_cn() {
    assert_eq!(eval_string(r#"replace_regex("\\d", "X", "a1b2")"#), "aXbX");
}

#[test]
fn to_toml_flat_int_key_cn() {
    let out = eval_string(r#"to_toml({ a => 1 })"#);
    assert!(out.contains("a = 1"), "got {:?}", out);
}

#[test]
fn yaml_encode_flat_int_key_cn() {
    let out = eval_string(r#"yaml_encode({ a => 1 })"#);
    assert!(out.contains("a:"), "got {:?}", out);
}

#[test]
fn from_json_object_one_key_cn() {
    assert_eq!(
        eval_string(r#"stringify(from_json("{\"a\": 1}"))"#),
        "+{a => 1}"
    );
}

#[test]
fn levenshtein_kitten_sitting_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", levenshtein("kitten", "sitting"))"#),
        "3"
    );
}

#[test]
fn jaccard_two_sets_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", jaccard([1, 2, 3], [2, 3, 4]))"#),
        "0.5"
    );
}

#[test]
fn sorensen_dice_two_numeric_lists_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", sorensen_dice([1, 2, 3], [2, 3, 4]))"#),
        "0.666667"
    );
}

#[test]
fn soundex_robert_cn() {
    assert_eq!(eval_string(r#"soundex("Robert")"#), "R163");
}

#[test]
fn metaphone_stephen_cn() {
    assert_eq!(eval_string(r#"metaphone("Stephen")"#), "STFHN");
}

#[test]
fn ngrams_two_abcd_cn() {
    assert_eq!(
        eval_string(r#"stringify(ngrams(2, "abcd"))"#),
        r#"("ab", "bc", "cd")"#
    );
}

#[test]
fn bigrams_alias_two_abcd_cn() {
    assert_eq!(
        eval_string(r#"stringify(bigrams("abcd"))"#),
        r#"("ab", "bc", "cd")"#
    );
}

#[test]
fn trigrams_alias_abcde_cn() {
    assert_eq!(
        eval_string(r#"stringify(trigrams("abcde"))"#),
        r#"("abc", "bcd", "cde")"#
    );
}

#[test]
fn transpose_variadic_rows_cn() {
    assert_eq!(
        eval_string(r#"stringify(transpose([1, 2], [3, 4]))"#),
        "([1, 3], [2, 4])"
    );
}

#[test]
fn transpose_single_nested_aoa_columns_wrapped_bug_cn() {
    assert_eq!(
        eval_string(r#"stringify(transpose([[1, 2], [3, 4]]))"#),
        "([[1, 2]], [[3, 4]])"
    );
}

#[test]
fn matrix_transpose_nested_aoa_cn() {
    assert_eq!(
        eval_string(r#"stringify(matrix_transpose([[1, 2], [3, 4]]))"#),
        "[[1, 3], [2, 4]]"
    );
}

#[test]
fn reverse_words_three_cn() {
    assert_eq!(
        eval_string(r#"reverse_words("one two three")"#),
        "three two one"
    );
}

#[test]
fn wrap_text_width_three_letters_cn() {
    assert_eq!(eval_string(r#"wrap_text("abcdef", 3)"#), "abcdef");
}

#[test]
fn word_count_trimmed_words_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", word_count("  a  b  c  "))"#),
        "3"
    );
}

#[test]
fn hamming_distance_strings_three_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", hamming_distance("karolin", "kathrin"))"#),
        "3"
    );
}

#[test]
fn matches_regex_anchor_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", matches_regex("abc", "^a"))"#),
        "1"
    );
}

#[test]
fn match_all_digit_pattern_first_cn() {
    assert_eq!(
        eval_string(r#"stringify(match_all("\\d", "a1b2"))"#),
        r#"("1", "2")"#
    );
}

#[test]
fn cumsum_one_through_four_cn() {
    assert_eq!(
        eval_string(r#"stringify(cumsum([1, 2, 3, 4]))"#),
        "(1, 3, 6, 10)"
    );
}

#[test]
fn correlation_perfect_linear_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", correlation([1, 2, 3], [2, 4, 6]))"#),
        "1"
    );
}

#[test]
fn similarity_abc_abd_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", similarity("abc", "abd"))"#),
        "0.666667"
    );
}

#[test]
fn common_prefix_ab_cn() {
    assert_eq!(eval_string(r#"common_prefix("abcdef", "abxy")"#), "ab");
}

#[test]
fn common_suffix_txt_cn() {
    assert_eq!(
        eval_string(r#"common_suffix("file.txt", "name.txt")"#),
        "e.txt"
    );
}

#[test]
fn dedent_strips_common_leading_spaces_cn() {
    assert_eq!(
        eval_string(r#"stringify(dedent("  hi\n    there"))"#),
        r#""hi\n  there""#
    );
}

#[test]
fn normalize_spaces_collapses_cn() {
    assert_eq!(eval_string(r#"normalize_spaces("  a   b  ")"#), "a b");
}

#[test]
fn is_anagram_dormitory_dirty_room_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", is_anagram("Dormitory", "Dirty room"))"#),
        "1"
    );
}

#[test]
fn is_pangram_classic_cn() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.0f", is_pangram("The quick brown fox jumps over the lazy dog"))"#
        ),
        "1"
    );
}

#[test]
fn median_odd_length_cn() {
    assert_eq!(eval_string(r#"sprintf("%.10g", median([1, 3, 10]))"#), "3");
}

#[test]
fn variance_small_list_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", variance([1, 2, 3, 4]))"#),
        "1.25"
    );
}

#[test]
fn stddev_sample_eight_values_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", stddev([2, 4, 4, 4, 5, 5, 7, 9]))"#),
        "2"
    );
}

#[test]
fn mode_val_multiset_cn() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", mode_val([1, 2, 2, 3]))"#),
        "2"
    );
}

#[test]
fn gcd_eighty_four_thirty_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", gcd(84, 30))"#), "6");
}

#[test]
fn lcm_twelve_eighteen_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", lcm(12, 18))"#), "36");
}

#[test]
fn oct_literal_seven_seven_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", oct("77"))"#), "63");
}

#[test]
fn hex_literal_ff_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", hex("ff"))"#), "255");
}

#[test]
fn ord_chr_roundtrip_capital_a_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", ord(chr(65)))"#), "65");
}

#[test]
fn is_prime_seventeen_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_prime(17))"#), "1");
}

#[test]
fn next_prev_prime_seventeen_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", next_prime(17))"#), "19");
    assert_eq!(eval_string(r#"sprintf("%.0f", prev_prime(17))"#), "13");
}

#[test]
fn factorial_six_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", factorial(6))"#), "720");
}

#[test]
fn fib_ten_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", fib(10))"#), "55");
}

#[test]
fn catalan_four_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", catalan(4))"#), "14");
}

#[test]
fn sum_divisors_twelve_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", sum_divisors(12))"#), "16");
}

#[test]
fn num_divisors_twelve_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", num_divisors(12))"#), "6");
}

#[test]
fn divisors_thirty_cn() {
    assert_eq!(
        eval_string(r#"stringify(divisors(30))"#),
        "(1, 2, 3, 5, 6, 10, 15, 30)"
    );
}

#[test]
fn is_square_thirty_six_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_square(36))"#), "1");
}

#[test]
fn is_perfect_twenty_eight_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_perfect(28))"#), "1");
}

#[test]
fn collatz_length_seven_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", collatz_length(7))"#), "16");
}

#[test]
fn int_sqrt_seventeen_floor_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", int(sqrt(17)))"#), "4");
}

#[test]
fn roman_roundtrip_fourteen_cn() {
    assert_eq!(eval_string(r#"sprintf("%.0f", roman_to_int("XIV"))"#), "14");
    assert_eq!(eval_string(r#"int_to_roman(14)"#), "XIV");
}

#[test]
fn rot13_hello_cn() {
    assert_eq!(eval_string(r#"rot13("uryyb")"#), "hello");
}

#[test]
fn caesar_shift_two_cn() {
    assert_eq!(eval_string(r#"caesar_shift("abc", 2)"#), "cde");
}
