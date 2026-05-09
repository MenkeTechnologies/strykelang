//! Behavior-pinning batch BP (2026-05-08): ~22 test fns — LCS-style strings, approximate equality,
//! path/File::Basename splits, Hamming/Levenshtein, comb/permy/zip/argsort/strftime/bbox-geohash,
//! weighted_mean, normalization + line counting, semver/IPv4/CIDR, hashes + quantifiers +
//! reductions/cycles/spaceship, histogram bins + pack hex.

use crate::common::*;

#[test]
fn string_overlap_bp() {
    assert_eq!(eval_string(r#"common_prefix("abcdef", "abcdgh")"#), "abcd");
    assert_eq!(
        eval_string(r#"common_suffix("file.txt", "name.txt")"#),
        "e.txt"
    );
    assert_eq!(
        eval_string(r#"longest_common_substring("abcdef", "bcd")"#),
        "bcd"
    );
}

#[test]
fn approx_eq_bp() {
    assert_eq!(eval_int("approx_eq(1.0, 1.0000001, 1e-6)"), 1);
    assert_eq!(
        eval_string(r#"sprintf("%.4f", approx_eq(3.0, 3.004, 0.01))"#),
        "1.0000"
    );
}

#[test]
fn path_components_bp() {
    assert_eq!(eval_string(r#"basename("/tmp/foo/bar.txt")"#), "bar.txt");
    assert_eq!(eval_string(r#"dirname("/tmp/foo/bar.txt")"#), "/tmp/foo");
    assert_eq!(
        eval_string(r#"join(",", fileparse("/tmp/foo/bar.tar.gz"))"#),
        "bar.tar.gz,/tmp/foo,"
    );
}

#[test]
fn realpath_nonempty_bp() {
    assert!(eval_int(r#"length(realpath("."))"#) >= 2);
}

#[test]
fn edit_distance_hamming_bp() {
    assert_eq!(eval_int(r#"edit_distance("kitten", "sitting")"#), 3);
    assert_eq!(eval_int(r#"hamming_distance_str("1010", "1001")"#), 2);
}

#[test]
fn combinations_permutations_bp() {
    assert_eq!(
        eval_string(r#"stringify(combinations(2, "a", "b", "c"))"#),
        r##"(["a", "b"], ["a", "c"], ["b", "c"])"##
    );
    assert_eq!(
        eval_string(r#"stringify(permutations(2, "a", "b"))"#),
        r##"(["a", "b"], ["b", "a"])"##
    );
}

#[test]
fn zip_fill_argsort_bp() {
    assert_eq!(
        eval_string(r#"stringify(zip_fill(0, [1, 2], [9]))"#),
        "([1, 9], [2, 0])"
    );
    assert_eq!(eval_string(r#"join(",", argsort(30, 10, 20))"#), "1,2,0");
}

#[test]
fn datetime_strftime_epoch_bp() {
    assert_eq!(
        eval_string(r#"datetime_strftime(0, "%Y-%m-%d UTC")"#),
        "1970-01-01 UTC"
    );
}

#[test]
fn bbox_geohash_bp() {
    assert_eq!(
        eval_string(r#"stringify(bounding_box([[0, 0], [2, 9], [10, 3]]))"#),
        "(0, 0, 10, 9)"
    );
    assert_eq!(
        eval_string(r#"geohash_encode(37.7749, -122.4194, 9)"#),
        "9q8yyk8yt"
    );
}

#[test]
fn weighted_mean_bp() {
    assert_eq!(
        eval_string(r#"sprintf("%.2f", weighted_mean([10, 40], [3, 7]))"#),
        "31.00"
    );
}

#[test]
fn whitespace_and_lines_bp() {
    assert_eq!(
        eval_string(r#"normalize_whitespace("  a  b\tc  ")"#),
        "a b c"
    );
    assert_eq!(eval_int(r#"line_count("a\nb\nc\n")"#), 3);
}

#[test]
fn semver_ipv4_cidr_bp() {
    assert_eq!(eval_int(r#"version_cmp("1.10.0", "1.9.0")"#), 1);
    assert_eq!(eval_int(r#"is_semver("1.2.3-rc.1")"#), 1);
    assert_eq!(
        eval_string(r#"stringify(ipv4_to_int("10.0.0.1"))"#),
        "167772161"
    );
    assert_eq!(eval_int(r#"is_valid_cidr("192.168.0.0/24")"#), 1);
}

#[test]
fn ord_chr_bp() {
    assert_eq!(eval_int(r#"ord("A")"#), 65);
    assert_eq!(eval_string(r#"chr(65)"#), "A");
}

#[test]
fn url_decode_bp() {
    assert_eq!(eval_string(r#"url_decode("a%20b")"#), "a b");
}

#[test]
fn quantifiers_all_any_none_bp() {
    assert_eq!(eval_int(r#"all { $_ > 0 } (1, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"any { $_ % 2 == 0 } (1, 3, 8)"#), 1);
    assert_eq!(eval_int(r#"none { $_ > 10 } (1, 4, 9)"#), 1);
}

#[test]
fn hash_eq_and_pairs_bp() {
    assert_eq!(eval_int(r#"hash_eq({ a => 1 }, { a => 1 })"#), 1);
    assert_eq!(eval_int(r#"hash_eq({ a => 1 }, { a => 2 })"#), 0);
    assert_eq!(eval_int(r#"scalar pairs_from_hash({ a => 1, b => 2 })"#), 2);
}

#[test]
fn sorted_keys_bp() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a cmp $b } keys({ b => 1, a => 2 }))"#),
        "a,b"
    );
}

#[test]
fn with_index_bp() {
    assert_eq!(
        eval_string(r#"stringify(with_index(qw(x y)))"#),
        r##"(["x", 0], ["y", 1])"##
    );
}

#[test]
fn histogram_bins_bp() {
    assert_eq!(
        eval_string(r#"join(",", histogram_bins(10, 20, 35, 41, 55, bin_width => 25))"#),
        "1,0,0,1,1,0,1,1,0,1"
    );
}

#[test]
fn list_repeat_batch_cycle_bp() {
    assert_eq!(
        eval_string(r#"join(",", repeat_list(2, 10, 20))"#),
        "10,20,10,20"
    );
    assert_eq!(
        eval_string(r#"stringify(batch(2, 1, 2, 3, 4, 5))"#),
        "([1, 2], [3, 4], [5])"
    );
    assert_eq!(eval_string(r#"join(",", interpose(99, 7, 8))"#), "7,99,8");
    assert_eq!(
        eval_string(r#"join(",", reductions({ $_[0] + $_[1] }, 1, 2, 3, 4))"#),
        "1,3,6,10"
    );
    assert_eq!(
        eval_string(r#"join(",", cycle_n(3, 40, 41))"#),
        "40,41,40,41,40,41"
    );
}

#[test]
fn uniqnum_and_spaceship_bp() {
    assert_eq!(eval_string(r#"join(",", uniqnum(1.0, "1", 2, 2))"#), "1,2");
    assert_eq!(eval_int("(10 <=> 20)"), -1);
    assert_eq!(eval_int(r#"scalar ("10" cmp "9")"#), -1);
}

#[test]
fn pack_hex_bp() {
    assert_eq!(eval_string(r#"hex_encode(pack("n", 4660))"#), "1234");
}
