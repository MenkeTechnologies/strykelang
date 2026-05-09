//! Behavior-pinning batch CL (2026-05-09): strings / encodings / phonetics, list–set helpers,
//! stats (variance, distances), angles (`d2r`/`r2d`), bits / rounding, hashes, JSON + URL
//! encode, regex bind — plus **`clamp(MIN,MAX,VAL)` vs mis-ordered triple** (**BUG-151**),
//! **`reverse` on string scalars** (**BUG-152**), **`hamming` → window, not distance** (**BUG-153**),
//! **`substr` UTF‑8 is byte‑indexed** (**BUG-154**).

use crate::common::*;

#[test]
fn chomp_strips_trailing_newline_cl() {
    assert_eq!(eval_string(r#"my $x = "a\n"; chomp($x); $x"#), "a");
}

#[test]
fn trim_ascii_ws_cl() {
    assert_eq!(eval_string(r#"trim("  x  ")"#), "x");
}

#[test]
fn words_splits_on_blank_lines_cl() {
    assert_eq!(
        eval_string(r#"join("|", words("a  b\nc"))"#),
        "a|b|c"
    );
}

#[test]
fn lc_uc_roundtrip_flags_cl() {
    assert_eq!(
        eval_string(r#"(lc("AbC") eq "abc" && uc("AbC") eq "ABC") ? "1" : "0""#),
        "1"
    );
}

#[test]
fn title_case_simple_clause_cl() {
    assert_eq!(eval_string(r#"title_case("a tale of two")"#), "A Tale Of Two");
}

#[test]
fn slugify_ascii_phrase_cl() {
    assert_eq!(eval_string(r#"slugify("Hello World!")"#), "hello-world");
}

#[test]
fn capitalize_first_grapheme_cl() {
    assert_eq!(eval_string(r#"capitalize("hello")"#), "Hello");
}

#[test]
fn reverse_str_unicode_safe_cl() {
    assert_eq!(eval_string(r#"reverse_str("abc")"#), "cba");
}

#[test]
fn reverse_variadic_three_ints_cl() {
    assert_eq!(eval_string(r#"stringify(reverse(1, 2, 3))"#), "(3, 2, 1)");
}

#[test]
fn reverse_single_inline_arrayref_identity_shape_cl() {
    assert_eq!(
        eval_string(r#"stringify(reverse([1, 2, 3]))"#),
        "[1, 2, 3]"
    );
}

#[test]
fn reverse_list_drains_bracket_list_cl() {
    assert_eq!(
        eval_string(r#"stringify(reverse_list([1, 2, 3]))"#),
        "(3, 2, 1)"
    );
}

#[test]
fn reverse_scalar_tail_expr_stringifies_reversed_cl() {
    assert_eq!(eval_string(r#"my $s = "abc"; reverse($s)"#), "cba");
}

#[test]
fn reverse_scalar_after_let_binding_reversed_cl() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; my $t = reverse($s); $t"#),
        "cba"
    );
}

#[test]
fn reverse_scalar_join_list_context_stays_forward_bug_cl() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; join("", reverse($s))"#),
        "abc"
    );
}

#[test]
fn char_at_grapheme_index_order_cl() {
    assert_eq!(eval_string(r#"char_at("αβγ", 1)"#), "β");
}

#[test]
fn code_point_at_second_beta_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", code_point_at("αβγ", 1))"#), "946");
}

#[test]
fn substr_ascii_slice_cl() {
    assert_eq!(eval_string(r#"substr("abc", 1, 1)"#), "b");
}

#[test]
fn substr_utf8_byte_window_one_grapheme_cl() {
    assert_eq!(eval_string(r#"substr("αβγ", 0, 2)"#), "α");
}

#[test]
fn substr_utf8_one_byte_mid_codepoint_empty_bug_cl() {
    assert_eq!(eval_string(r#"substr("αβγ", 1, 1)"#), "");
}

#[test]
fn rot13_roundtrip_cl() {
    assert_eq!(eval_string(r#"rot13("Uryyb")"#), "Hello");
}

#[test]
fn soundex_ashcraft_cl() {
    assert_eq!(eval_string(r#"soundex("Ashcraft")"#), "A226");
}

#[test]
fn metaphone_knight_cl() {
    assert_eq!(eval_string(r#"metaphone("knight")"#), "NT");
}

#[test]
fn repeat_string_factor_cl() {
    assert_eq!(eval_string(r#"repeat("ab", 3)"#), "ababab");
}

#[test]
fn tr_lower_to_upper_cl() {
    assert_eq!(
        eval_string(r#"my $x = "abc"; $x =~ tr/a-z/A-Z/; $x"#),
        "ABC"
    );
}

#[test]
fn split_join_on_spaces_cl() {
    assert_eq!(
        eval_string(r#"join("|", split(" ", "a b c"))"#),
        "a|b|c"
    );
}

#[test]
fn bind_match_all_digits_cl() {
    assert_eq!(
        eval_string(r#"(("007" =~ qr{^\d+$}) ? "1" : "0")"#),
        "1"
    );
}

#[test]
fn bind_match_rejects_letter_cl() {
    assert_eq!(
        eval_string(r#"(("7a" =~ qr{^\d+$}) ? "1" : "0")"#),
        "0"
    );
}

#[test]
fn json_encode_decode_int_roundtrip_cl() {
    assert_eq!(
        eval_string(r#"int(json_decode(json_encode({ x => 42 }))->{x})"#),
        "42"
    );
}

#[test]
fn url_encode_space_percent20_cl() {
    assert_eq!(eval_string(r#"url_encode("a b")"#), "a%20b");
}

#[test]
fn hex_ff_roundtrip_cl() {
    assert_eq!(
        eval_string(r#"(to_hex(255) eq "ff" && int(from_hex("ff")) == 255) ? "1" : "0""#),
        "1"
    );
}

#[test]
fn oct_literal_three_sevens_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", int(oct("777")))"#), "511");
}

#[test]
fn hypot_34_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", hypot(3, 4))"#), "5");
}

#[test]
fn gcd_lcm_small_cl() {
    assert_eq!(
        eval_string(r#"(sprintf("%.0f", gcd(84, 30)) eq "6" && sprintf("%.0f", lcm(4, 6)) eq "12") ? "1" : "0""#),
        "1"
    );
}

#[test]
fn copysign_flips_with_negative_second_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", copysign(1, -0.5))"#), "-1");
}

#[test]
fn log10_log2_exp_edge_cl() {
    assert_eq!(
        eval_string(
            r#"(sprintf("%.10g", log10(100)) eq "2" && sprintf("%.10g", log2(8)) eq "3" && sprintf("%.10g", exp(0)) eq "1") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn cbrt_27_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", cbrt(27))"#), "3");
}

#[test]
fn ceil_floor_simple_cl() {
    assert_eq!(
        eval_string(
            r#"(sprintf("%.10g", ceil(1.1)) eq "2" && sprintf("%.10g", floor(1.9)) eq "1") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn trunc_toward_zero_negative_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", trunc(-1.8))"#), "-1");
}

#[test]
fn sign_triple_cl() {
    assert_eq!(
        eval_string(r#"join(",", sprintf("%.0f", sign(-2)), sprintf("%.0f", sign(0)), sprintf("%.0f", sign(3)))"#),
        "-1,0,1"
    );
}

#[test]
fn abs_float_negative_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", abs(-3.5))"#), "3.5");
}

#[test]
fn d2r_180_cl() {
    assert_eq!(eval_string(r#"sprintf("%.12g", d2r(180))"#), "3.14159265359");
}

#[test]
fn r2d_pi_cl() {
    assert_eq!(eval_string(r#"sprintf("%.12g", r2d(3.141592653589793))"#), "180");
}

#[test]
fn shl_shr_int_cl() {
    assert_eq!(
        eval_string(r#"(sprintf("%.0f", shl(1, 4)) eq "16" && sprintf("%.0f", shr(16, 2)) eq "4") ? "1" : "0""#),
        "1"
    );
}

#[test]
fn bit_xor_nibble_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_xor(15, 3))"#), "12");
}

#[test]
fn bit_not_zero_is_negative_one_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_not(0))"#), "-1");
}

#[test]
fn round_half_up_two_decimals_cl() {
    assert_eq!(eval_string(r#"sprintf("%.2f", round(3.14159, 2))"#), "3.14");
}

#[test]
fn variance_and_stddev_sample_cl() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g %.10g", variance([2, 4, 4, 4, 5, 5, 7, 9]), stddev([2, 4, 4, 4, 5, 5, 7, 9]))"#
        ),
        "4 2"
    );
}

#[test]
fn levenshtein_kitten_sitting_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", levenshtein("kitten", "sitting"))"#), "3");
}

#[test]
fn jaccard_three_each_cl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", jaccard([1, 2, 3], [2, 3, 4]))"#),
        "0.5"
    );
}

#[test]
fn median_odd_three_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", median([1, 3, 9]))"#), "3");
}

#[test]
fn percentile_fifty_of_five_cl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", percentile(50, [1, 2, 3, 4, 5]))"#),
        "3"
    );
}

#[test]
fn dot_product_small_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", dot_product([1, 2], [3, 4]))"#), "11");
}

#[test]
fn manhattan_taxicab_cl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", manhattan_distance([0, 0], [3, 4]))"#),
        "7"
    );
}

#[test]
fn euclidean_34_cl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", euclidean_distance([0, 0], [3, 4]))"#),
        "5"
    );
}

#[test]
fn hamming_distance_bit_flip_one_cl() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", hamming_distance("1101", "1001"))"#),
        "1"
    );
}

#[test]
fn bool_to_int_truthiness_cl() {
    assert_eq!(
        eval_string(
            r#"(sprintf("%.0f", bool_to_int(42)) eq "1" && sprintf("%.0f", bool_to_int(0)) eq "0" && sprintf("%.0f", bool_to_int("")) eq "0") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn base64_roundtrip_ascii_cl() {
    assert_eq!(
        eval_string(r#"base64_decode(base64_encode("stryke"))"#),
        "stryke"
    );
}

#[test]
fn invert_hash_swaps_pairs_cl() {
    assert_eq!(
        eval_string(r#"stringify(invert({ a => 1, b => 2 }))"#),
        r#"+{1 => "a", 2 => "b"}"#
    );
}

#[test]
fn merge_hash_shallow_union_cl() {
    assert_eq!(
        eval_string(r#"stringify(merge_hash({ a => 1 }, { b => 2 }))"#),
        "+{a => 1, b => 2}"
    );
}

#[test]
fn array_difference_sorted_strings_cl() {
    assert_eq!(
        eval_string(r#"stringify(array_difference([1, 2, 3], [2]))"#),
        "(\"1\", \"3\")"
    );
}

#[test]
fn symmetric_diff_pair_cl() {
    assert_eq!(
        eval_string(r#"stringify(symmetric_diff([1, 2], [2, 3]))"#),
        "(\"1\", \"3\")"
    );
}

#[test]
fn clamp_scalar_inside_range_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", clamp(0, 10, 11))"#), "10");
}

#[test]
fn clamp_value_min_max_order_misread_as_min_max_list_bug_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", clamp(11, 0, 10))"#), "11");
}

#[test]
fn clamp_list_bounds_per_value_cl() {
    assert_eq!(
        eval_string(r#"stringify(clamp_list(0, 10, 5, 15, -1))"#),
        "(5, 10, 0)"
    );
}

#[test]
fn sort_keys_hash_alpha_cl() {
    assert_eq!(
        eval_string(r#"join(",", sort keys { b => 2, a => 1 })"#),
        "a,b"
    );
}

// ── Extra numeric / combinatorial pins (same batch) ───────────────────────────

#[test]
fn acos_minus_one_pi_cl() {
    assert_eq!(eval_string(r#"sprintf("%.9g", acos(-1))"#), "3.14159265");
}

#[test]
fn asin_one_half_pi_cl() {
    assert_eq!(eval_string(r#"sprintf("%.9g", asin(1))"#), "1.57079633");
}

#[test]
fn atan2_one_one_quarter_pi_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", atan2(1, 1))"#), "0.7853981634");
}

#[test]
fn tan_forty_five_degrees_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", tan(d2r(45)))"#), "1");
}

#[test]
fn min_max_float_triple_cl() {
    assert_eq!(
        eval_string(
            r#"(sprintf("%.10g", min(3.0, 9.0, -1.0)) eq "-1" && sprintf("%.10g", max(3.0, 9.0, -1.0)) eq "9") ? "1" : "0""#
        ),
        "1"
    );
}

#[test]
fn factorial_five_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", factorial(5))"#), "120");
}

#[test]
fn is_prime_seventeen_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_prime(17))"#), "1");
}

#[test]
fn next_prime_after_fourteen_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", next_prime(14))"#), "17");
}

#[test]
fn binomial_five_choose_two_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", binomial(5, 2))"#), "10");
}

#[test]
fn erf_zero_cl() {
    assert_eq!(eval_string(r#"sprintf("%.10g", erf(0))"#), "0");
}

#[test]
fn digital_root_thousand_two_thirty_four_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", digital_root(1234))"#), "1");
}

#[test]
fn collatz_length_ten_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", collatz_length(10))"#), "6");
}

#[test]
fn fibonacci_tenth_index_cl() {
    assert_eq!(eval_string(r#"sprintf("%.0f", fibonacci(10))"#), "55");
}

#[test]
fn harmonic_mean_three_values_cl() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic_mean([1, 2, 4]))"#),
        "1.714285714"
    );
}
