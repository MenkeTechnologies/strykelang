//! Behavior-pinning batch CP (2026-05-09): string / digest (**`xxh`**, **`blake`**, **`sha1`–`sha512`**, **`ripemd`**, **`murmur`**),
//! **`crc16_ccitt`**, **`sdbm`**, **`jenkins`**, vector distances, **planar scalar** geometry (**`slope`**, **`midpoint`**, **`chebyshev_distance`**)
//! vs **`distance` / `manhattan_distance` / `euclidean_distance`** (**BUG-162**), list scans, path **`basename`/`dirname`**, **`haversine`**, correlations, **`max_list`/`min_list`**.

use crate::common::*;

#[test]
fn repeat_string_triple_cp() {
    assert_eq!(eval_string(r#"repeat("ab", 3)"#), "ababab");
}

#[test]
fn perl_cmp_strings_lexicographic_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", ("a" cmp "b"))"#), "-1");
    assert_eq!(
        eval_string(r#"sprintf("%.0f", ("item2" cmp "item10"))"#),
        "1"
    );
}

#[test]
fn unpack_pack_float_roundtrip_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", scalar(unpack("f", pack("f", 1.5))))"#),
        "1.5"
    );
}

#[test]
fn crc16_ccitt_ascii_triple_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", crc16_ccitt("abc"))"#),
        "20810"
    );
}

#[test]
fn sdbm_hash_ascii_triple_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", sdbm_hash("abc"))"#),
        "807794786"
    );
}

#[test]
fn jenkins_one_at_a_time_ascii_triple_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", jenkins_one_at_a_time("abc"))"#),
        "3977453403"
    );
}

#[test]
fn murmur3_32_hex_seed_zero_cp() {
    assert_eq!(eval_string(r#"murmur3("abc", 0)"#), "b3dd93fa");
}

#[test]
fn murmur3_32_hex_seed_one_cp() {
    assert_eq!(eval_string(r#"murmur3("abc", 1)"#), "aa75e9ff");
}

#[test]
fn murmurhash3_x32_builtin_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", murmurhash3_x32("abc", 0))"#),
        "3017643002"
    );
}

#[test]
fn xxh32_hex_abc_cp() {
    assert_eq!(eval_string(r#"xxh32("abc")"#), "32d153ff");
}

#[test]
fn xxh64_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"xxh64("abc")"#),
        "44bc2cf5ad770999"
    );
}

#[test]
fn blake3_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"blake3("abc")"#),
        "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
    );
}

#[test]
fn sha1_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"sha1("abc")"#),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
}

#[test]
fn ripemd160_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"ripemd160("abc")"#),
        "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
    );
}

#[test]
fn distance_vec_two_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", distance([0, 0], [3, 4]))"#),
        "5"
    );
}

#[test]
fn manhattan_distance_vec_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", manhattan_distance([-1, 2], [3, -2]))"#),
        "8"
    );
}

#[test]
fn chebyshev_distance_four_scalars_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_distance(0, 0, 3, 4))"#),
        "4"
    );
}

#[test]
fn chebyshev_two_vectors_coerces_to_zero_bug_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chebyshev_distance([0, 0], [3, 4]))"#),
        "0"
    );
}

#[test]
fn slope_four_coordinates_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", slope(0, 0, 1, 2))"#),
        "2"
    );
}

#[test]
fn slope_with_two_vector_args_vertical_line_inf_bug_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", slope([1, 2, 3], [2, 4, 6]))"#),
        "inf"
    );
}

#[test]
fn midpoint_four_coordinates_cp() {
    assert_eq!(
        eval_string(r#"stringify(midpoint(0, 0, 4, 6))"#),
        "(2, 3)"
    );
}

#[test]
fn triangle_hypotenuse_three_four_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", triangle_hypotenuse(3, 4))"#),
        "5"
    );
}

#[test]
fn polygon_area_axis_square_cp() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", polygon_area([[0, 0], [2, 0], [2, 2], [0, 2]]))"#,
        ),
        "4"
    );
}

#[test]
fn sphere_volume_radius_three_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", sphere_volume(3))"#),
        "113.0973355"
    );
}

#[test]
fn cosine_similarity_unit_vectors_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", cosine_similarity([1, 0], [1, 1]))"#),
        "0.7071067812"
    );
}

#[test]
fn angle_between_deg_forty_five_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", angle_between_deg(1, 1))"#),
        "45"
    );
}

#[test]
fn d2r_and_r2d_roundtrip_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", d2r(180))"#),
        "3.141592654"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", r2d(1.5707963267948966))"#),
        "90"
    );
}

#[test]
fn scan_numeric_prefix_sums_cp() {
    assert_eq!(
        eval_string(r#"stringify(scan(1, 2, 3, 4))"#),
        "(1, 3, 6, 10)"
    );
}

#[test]
fn collapse_whitespace_cp() {
    assert_eq!(
        eval_string(r#"collapse_whitespace("a  b\t\tc")"#),
        "a b c"
    );
}

#[test]
fn has_duplicates_multiset_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", has_duplicates([1, 2, 2, 3]))"#),
        "1"
    );
}

#[test]
fn duplicate_count_multiset_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", duplicate_count([1, 2, 2, 3]))"#),
        "1"
    );
}

#[test]
fn nub_stable_unique_cp() {
    assert_eq!(
        eval_string(r#"stringify(nub([1, 2, 2, 3, 1]))"#),
        "(1, 2, 3)"
    );
}

#[test]
fn covariance_sample_lists_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", covariance([1, 2, 3], [1, 2, 4]))"#),
        "1.5"
    );
}

#[test]
fn correlation_not_perfect_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", correlation([1, 2, 3], [2, 4, 7]))"#),
        "0.9933992678"
    );
}

#[test]
fn before_n_and_after_n_cp() {
    assert_eq!(
        eval_string(r#"stringify(before_n(2, 1, 2, 3, 4))"#),
        "(1, 2)"
    );
    assert_eq!(
        eval_string(r#"stringify(after_n(2, 1, 2, 3, 4))"#),
        "(3, 4)"
    );
}

#[test]
fn hex_encode_decode_roundtrip_cp() {
    assert_eq!(
        eval_string(r#"hex_decode(hex_encode("ab"))"#),
        "ab"
    );
}

#[test]
fn ceil_and_floor_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", ceil(1.2))"#), "2");
    assert_eq!(eval_string(r#"sprintf("%.0f", floor(-1.2))"#), "-2");
}

#[test]
fn round_two_decimal_places_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", round(1.23456, 2))"#),
        "1.23"
    );
}

#[test]
fn words_split_trim_cp() {
    assert_eq!(
        eval_string(r#"stringify(words("  a  bb  c  "))"#),
        r#"("a", "bb", "c")"#
    );
}

#[test]
fn is_sorted_nondecreasing_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", is_sorted([1, 2, 3]))"#),
        "1"
    );
}

#[test]
fn popcount_byte_wide_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", popcount(255))"#), "8");
}

#[test]
fn is_power_of_two_sixteen_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_power_of_two(16))"#), "1");
}

#[test]
fn pow2_five_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", pow2(5))"#), "32");
}

#[test]
fn count_substring_ab_occurrences_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", count_substring("ababab", "ab"))"#),
        "3"
    );
}

#[test]
fn sha224_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"sha224("abc")"#),
        "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7"
    );
}

#[test]
fn trim_ascii_whitespace_cp() {
    assert_eq!(eval_string(r#"trim("  x  ")"#), "x");
}

#[test]
fn parse_float_decimal_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", parse_float("3.5"))"#),
        "3.5"
    );
}

#[test]
fn from_hex_literal_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", from_hex("10"))"#), "16");
}

#[test]
fn sorted_nums_lex_numeric_cp() {
    assert_eq!(
        eval_string(r#"stringify(sorted_nums(10, 2, 3))"#),
        "(2, 3, 10)"
    );
}

#[test]
fn euclidean_distance_matches_alias_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", euclidean_distance([0, 0], [3, 4]))"#),
        "5"
    );
}

#[test]
fn matrix_det_three_by_three_cp() {
    assert_eq!(
        eval_string(
            r#"sprintf("%.0f", det([[2, 0, 0], [0, 3, 0], [0, 0, 4]]))"#,
        ),
        "24"
    );
}

#[test]
fn slice_substr_unicode_safe_chars_cp() {
    assert_eq!(eval_string(r#"substr("abcdef", 1, 3)"#), "bcd");
}

#[test]
fn split_fixed_limit_cp() {
    assert_eq!(
        eval_string(r#"stringify(split(":", "a:b:c:d", 3))"#),
        r#"("a", "b", "c:d")"#
    );
}

#[test]
fn blake2s_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"blake2s("abc")"#),
        "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982"
    );
}

#[test]
fn sha384_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"sha384("abc")"#),
        "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
    );
}

#[test]
fn basename_and_dirname_posix_cp() {
    assert_eq!(eval_string(r#"basename("/tmp/foo.bar")"#), "foo.bar");
    assert_eq!(eval_string(r#"dirname("/tmp/foo.bar")"#), "/tmp");
}

#[test]
fn haversine_one_degree_lon_at_equator_cp() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", haversine(0, 0, 0, 1))"#),
        "111.195"
    );
}

#[test]
fn max_list_and_min_list_variadic_cp() {
    assert_eq!(eval_string(r#"sprintf("%.0f", max_list(1, 5, 3))"#), "5");
    assert_eq!(eval_string(r#"sprintf("%.0f", min_list(1, 5, 3))"#), "1");
}

#[test]
fn sha512_hex_abc_cp() {
    assert_eq!(
        eval_string(r#"sha512("abc")"#),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
}
