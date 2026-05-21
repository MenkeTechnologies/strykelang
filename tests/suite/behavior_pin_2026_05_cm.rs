//! Behavior-pinning batch CM (2026-05-09): checksums / digests, CSV helpers, HTML encodings,
//! float predicates, interpolation (`lerp`/`smoothstep`), iterators (`iota`), numeric ranges, modular
//! arithmetic, divisor / totient, bases (`sprintf %b`), strings (`starts_with`, `pad_*`, `center`),
//! matrices (`det`/`matrix_trace`/`l2_norm`), stats (`geometric_mean`, `quantile`), semver / IPv4 /
//! popcount, gzip round-trip, string metrics — plus **`seq`** (**BUG-156**) and **`crc32`** tails (**BUG-157**).

use crate::common::*;

#[test]
fn crc32_ascii_triple_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", crc32("abc"))"#), "891568578");
}

#[test]
fn adler32_ascii_triple_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", adler32("abc"))"#),
        "38600999"
    );
}

#[test]
fn md5_hex_abc_cm() {
    assert_eq!(
        eval_string(r#"md5("abc")"#),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}

#[test]
fn sha256_hex_abc_cm() {
    assert_eq!(
        eval_string(r#"sha256("abc")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn from_csv_line_quoted_field_cm() {
    assert_eq!(
        eval_string(r#"stringify(from_csv_line("a,\"b,c\",d"))"#),
        r#"("a", "b,c", "d")"#
    );
}

#[test]
fn to_csv_line_two_columns_cm() {
    assert_eq!(eval_string(r#"to_csv_line(qw(a b))"#), "a,b");
}

#[test]
fn strip_html_tags_cm() {
    assert_eq!(eval_string(r#"strip_html("<p>x</p>")"#), "x");
}

#[test]
fn html_encode_entities_cm() {
    assert_eq!(eval_string(r#"html_encode("<&>")"#), "&lt;&amp;&gt;");
}

#[test]
fn is_inf_positive_overflow_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_inf(1e400))"#), "1");
}

#[test]
fn is_nan_literal_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_nan(NaN))"#), "1");
}

#[test]
fn lerp_quarter_cm() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lerp(0, 10, 0.25))"#), "2.5");
}

#[test]
fn smoothstep_mid_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", smoothstep(0, 1, 0.5))"#),
        "0.5"
    );
}

#[test]
fn iota_three_cm() {
    assert_eq!(eval_string(r#"stringify(iota(3))"#), "(0, 1, 2)");
}

#[test]
fn seq_two_args_only_first_used_bug_cm() {
    assert_eq!(eval_string(r#"stringify(seq(2, 5))"#), "2");
}

#[test]
fn range_inclusive_two_five_cm() {
    assert_eq!(eval_string(r#"stringify(range(2, 5))"#), "(2, 3, 4, 5)");
}

#[test]
fn powmod_square_mod_prime_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", powmod(7, 2, 13))"#), "10");
}

#[test]
fn mod_inv_three_mod_eleven_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", mod_inv(3, 11))"#), "4");
}

#[test]
fn divisors_twelve_cm() {
    assert_eq!(
        eval_string(r#"stringify(divisors(12))"#),
        "(1, 2, 3, 4, 6, 12)"
    );
}

#[test]
fn euler_totient_nine_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", euler_totient(9))"#), "6");
}

#[test]
fn sprintf_binary_five_cm() {
    assert_eq!(eval_string(r#"sprintf("%b", 5)"#), "101");
}

#[test]
fn map_chr_sixty_five_sixty_six_cm() {
    assert_eq!(eval_string(r#"join("", map { chr($_) } (65, 66))"#), "AB");
}

#[test]
fn starts_with_foo_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", starts_with("foobar", "foo"))"#),
        "1"
    );
}

#[test]
fn ends_with_bar_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", ends_with("foobar", "bar"))"#),
        "1"
    );
}

#[test]
fn contains_substr_oba_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", contains("foobar", "oba"))"#),
        "1"
    );
}

#[test]
fn index_substr_oba_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", index("foobar", "oba"))"#),
        "2"
    );
}

#[test]
fn det_two_by_two_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", det([[1, 2], [3, 4]]))"#),
        "-2"
    );
}

#[test]
fn matrix_trace_two_by_two_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", matrix_trace([[1, 2], [3, 4]]))"#),
        "5"
    );
}

#[test]
fn l2_norm_three_four_cm() {
    assert_eq!(eval_string(r#"sprintf("%.10g", l2_norm([3, 4]))"#), "5");
}

#[test]
fn sort_numeric_cmp_join_cm() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } (10, 2, 3))"#),
        "2,3,10"
    );
}

#[test]
fn geometric_mean_cube_root_triple_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", geometric_mean([1, 3, 9]))"#),
        "3"
    );
}

#[test]
fn quantile_quarter_five_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", quantile(0.25, [1, 2, 3, 4, 5]))"#),
        "0.25"
    );
}

#[test]
fn pad_left_zeros_cm() {
    assert_eq!(eval_string(r#"pad_left("42", 5, "0")"#), "00042");
}

#[test]
fn pad_right_zeros_cm() {
    assert_eq!(eval_string(r#"pad_right("42", 5, "0")"#), "42000");
}

#[test]
fn center_dashes_cm() {
    assert_eq!(eval_string(r#"center("ab", 6, "-")"#), "--ab--");
}

#[test]
fn version_cmp_newer_patch_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", version_cmp("1.10.0", "1.9.0"))"#),
        "1"
    );
}

#[test]
fn is_semver_valid_triple_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_semver("1.2.3"))"#), "1");
}

#[test]
fn is_semver_rejects_v_prefix_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_semver("v1.2"))"#), "0");
}

#[test]
fn ipv4_to_int_loopback_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", ipv4_to_int("127.0.0.1"))"#),
        "2130706433"
    );
}

#[test]
fn int_to_ipv4_roundtrip_cm() {
    assert_eq!(
        eval_string(r#"int_to_ipv4(ipv4_to_int("127.0.0.1"))"#),
        "127.0.0.1"
    );
}

#[test]
fn popcount_seven_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", popcount(7))"#), "3");
}

#[test]
fn gzip_gunzip_roundtrip_ascii_cm() {
    assert_eq!(eval_string(r#"gunzip(gzip("hello"))"#), "hello");
}

#[test]
fn atbash_lowercase_cm() {
    assert_eq!(eval_string(r#"atbash("abc")"#), "zyx");
}

#[test]
fn edit_distance_ab_bc_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", edit_distance("ab", "bc"))"#),
        "2"
    );
}

#[test]
fn sorensen_dice_disjoint_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sorensen_dice("night", "nacht"))"#),
        "0"
    );
}

#[test]
fn is_blank_whitespace_only_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_blank("  "))"#), "1");
}

#[test]
fn is_empty_string_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_empty(""))"#), "1");
}

#[test]
fn byte_length_greek_alpha_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", byte_length("α"))"#), "2");
}

#[test]
fn char_length_three_greek_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", char_length("αβγ"))"#), "3");
}

#[test]
fn xor_three_strings_charwise_cm() {
    assert_eq!(
        eval_string(r#"string_xor(string_xor("abc", "abc"), "xyz")"#),
        "xyz"
    );
}

#[test]
fn not_elem_missing_from_list_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", not_elem(5, [1, 2, 3]))"#),
        "1"
    );
}

#[test]
fn contains_elem_positive_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", contains_elem(2, [1, 2, 3]))"#),
        "1"
    );
}

#[test]
fn pluralize_cat_count_two_cm() {
    assert_eq!(eval_string(r#"pluralize("cat", 2)"#), "cats");
}

#[test]
fn title_case_single_word_cm() {
    assert_eq!(eval_string(r#"title_case("perl")"#), "Perl");
}

#[test]
fn json_bool_wire_true_cm() {
    assert_eq!(eval_string(r#"(to_json(true) =~ /true/) ? "1" : "0""#), "1");
}

#[test]
fn uri_escape_space_percent20_cm() {
    assert_eq!(eval_string(r#"uri_escape("a b")"#), "a%20b");
}

#[test]
fn crc32_separate_args_equals_concat_cm() {
    // crc32 hashes every positional arg, so the per-arg digest matches the
    // concatenated single-string digest.
    assert_eq!(
        eval_string(r#"(crc32("ab") == crc32("a", "b")) ? "1" : "0""#),
        "1"
    );
}

#[test]
fn matrix_mul_identity_two_cm() {
    assert_eq!(
        eval_string(r#"stringify(matrix_mul([[1, 0], [0, 1]], [[2, 3], [4, 5]]))"#),
        "([2, 3], [4, 5])"
    );
}

#[test]
fn kronecker_one_one_two_cm() {
    assert_eq!(
        eval_string(r#"stringify(kronecker_product([[1, 2]], [[10]]))"#),
        "(10, 20)"
    );
}

#[test]
fn argmax_three_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", argmax(3, 1, 9, 2))"#), "2");
}

#[test]
fn argmin_three_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", argmin(3, 1, 9, 2))"#), "1");
}

#[test]
fn clamp01_unit_interval_helper_cm() {
    assert_eq!(eval_string(r#"sprintf("%.10g", clamp(0, 1, 1.5))"#), "1");
}

#[test]
fn nth_root_eight_three_cm() {
    assert_eq!(eval_string(r#"sprintf("%.10g", nth_root(8, 3))"#), "2");
}

#[test]
fn is_power_of_two_sixteen_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_power_of_two(16))"#), "1");
}

#[test]
fn popcount_byte_max_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", popcount(255))"#), "8");
}

#[test]
fn set_bit_three_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_set(0, 3))"#), "8");
}

#[test]
fn clear_bit_on_fifteen_cm() {
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_clear(15, 1))"#), "13");
}

#[test]
fn fnv1a_hash_abc_decimal_cm() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", fnv1a_hash("abc"))"#),
        "-1792535898324117760"
    );
}

#[test]
fn ordinalize_forty_two_cm() {
    assert_eq!(eval_string(r#"ordinalize(42)"#), "42nd");
}
