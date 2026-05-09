//! Behavior-pinning batch BR (2026-05-08): list reshaping (take/tail/drop ordering, interleave,
//! concat, flatten), sliding_pairs, string chunking (trim/words/chars/digits/numbers), predicates,
//! aggregates (frequencies/pfrequencies/count_by/uniq sort), map-style math (squared/cubed/expt,
//! stddev), selection (min_by/max_by, grep, first_or, compact), partition / zip_with blocks.

use crate::common::*;

#[test]
fn interleave_concat_br() {
    assert_eq!(
        eval_string(r#"join(",", interleave(1, 2, 10, 20))"#),
        "1,2,10,20"
    );
    assert_eq!(
        eval_string(r#"join(",", concat(7, 8, 80, 90))"#),
        "7,8,80,90"
    );
}

#[test]
fn take_tail_drop_list_then_count_br() {
    assert_eq!(eval_string(r#"join(",", take(1, 2, 3, 4, 5, 3))"#), "1,2,3");
    assert_eq!(eval_string(r#"join(",", tail(1, 2, 3, 4, 5, 3))"#), "3,4,5");
    assert_eq!(eval_string(r#"join(",", drop(1, 2, 3, 4, 5, 2))"#), "3,4,5");
}

#[test]
fn normalize_flatten_br() {
    assert_eq!(
        eval_string(r#"stringify(normalize(-8, 0, 8))"#),
        "(0, 0.5, 1)"
    );
    assert_eq!(
        eval_string(r#"stringify(flatten(9, [10, [11]], 12))"#),
        "(9, 10, [11], 12)"
    );
}

#[test]
fn trim_words_chars_digits_numbers_br() {
    assert_eq!(eval_string(r#"trim("  xy  ")"#), "xy");
    assert_eq!(
        eval_string(r#"stringify(words("aa bb\tcc"))"#),
        "(\"aa\", \"bb\", \"cc\")"
    );
    assert_eq!(eval_string(r#"stringify(chars("π"))"#), "(\"π\")");
    assert_eq!(eval_string(r#"join(",", digits("z9y8x7"))"#), "9,8,7");
    assert_eq!(eval_string(r#"join(",", numbers("p12q-3r"))"#), "12,-3");
}

#[test]
fn stringify_with_index_br() {
    assert_eq!(
        eval_string(r#"stringify(with_index(100, 200))"#),
        "([100, 0], [200, 1])"
    );
}

#[test]
fn frequencies_pfrequencies_br() {
    assert_eq!(
        eval_string(r#"stringify(frequencies("x", "y", "x", "z", "x"))"#),
        "+{x => 3, y => 1, z => 1}"
    );
    assert_eq!(
        eval_string(r#"scalar(keys(%{ pfrequencies("a", "b", "a") }))"#),
        "2"
    );
}

#[test]
fn compact_first_or_br() {
    assert_eq!(eval_string(r#"join(",", compact(undef, "", 0, 7))"#), "0,7");
    assert_eq!(eval_int(r#"first_or(-9, ())"#), -9);
    assert_eq!(eval_int(r#"first_or(-9, 42, 43)"#), 42);
}

#[test]
fn partition_block_br() {
    assert_eq!(
        eval_string(
            r#"my @r = partition { _ % 2 == 0 } (11, 12, 13, 14, 15);
               join("|", join(",", @{$r[0]}), join(",", @{$r[1]}))"#
        ),
        "12,14|11,13,15"
    );
}

#[test]
fn count_by_bucket_br() {
    assert_eq!(
        eval_string(
            r#"my %h = %{ count_by { int($_ / 10) } (5, 14, 22, 31) };
               join(",", sort { $a <=> $b } keys(%h))"#
        ),
        "0,1,2,3"
    );
}

#[test]
fn zip_with_add_br() {
    assert_eq!(
        eval_string(r#"my @r = zip_with { $_[0] + $_[1] } [3, 4], [300, 400]; join(",", @r)"#),
        "303,404"
    );
}

#[test]
fn squared_cubed_expt_br() {
    assert_eq!(eval_int(r#"int(squared(12))"#), 144);
    assert_eq!(eval_int(r#"int(cubed(-3))"#), -27);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", expt(2.25, 3))"#),
        "11.390625"
    );
}

#[test]
fn stddev_three_points_br() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", stddev(100, 200, 300))"#),
        "81.649658"
    );
}

#[test]
fn capitalize_swap_repeat_br() {
    assert_eq!(eval_string(r#"capitalize("ruby")"#), "Ruby");
    assert_eq!(eval_string(r#"swap_case("LoRem")"#), "lOrEM");
    assert_eq!(eval_string(r#"repeat("--", 4)"#), "--------");
}

#[test]
fn substring_predicates_br() {
    assert_eq!(eval_int(r#"starts_with("abcde", "abc")"#), 1);
    assert_eq!(eval_int(r#"ends_with("abcde", "cde")"#), 1);
    assert_eq!(eval_int(r#"contains("abcde", "bcd")"#), 1);
    assert_eq!(eval_int(r#"length(repeat("*", 7))"#), 7);
}

#[test]
fn is_blank_caps_kind_br() {
    assert_eq!(eval_int(r#"is_blank("   ")"#), 1);
    assert_eq!(eval_int(r#"is_blank("x")"#), 0);
    assert_eq!(eval_int(r#"is_upper("AB")"#), 1);
    assert_eq!(eval_int(r#"is_lower("ab")"#), 1);
    assert_eq!(eval_int(r#"is_alpha("Ab")"#), 1);
}

#[test]
fn grep_numeric_br() {
    assert_eq!(
        eval_string(r#"join(",", grep { $_ >= 10 } (3, 10, 9, 11))"#),
        "10,11"
    );
}

#[test]
fn uniq_sort_default_br() {
    assert_eq!(
        eval_string(r#"join(",", uniq(5, 1, 5, 2, 1, 9))"#),
        "5,1,2,9"
    );
    assert_eq!(eval_string(r#"join(",", sort (9, -1, 4))"#), "-1,4,9");
}

#[test]
fn min_by_max_by_length_br() {
    assert_eq!(
        eval_string(r#"my @r = min_by { length($_) } ("zzz", "a", "bb"); $r[0]"#),
        "a"
    );
    assert_eq!(
        eval_string(r#"my @r = max_by { length($_) } ("zzz", "a", "bb"); $r[0]"#),
        "zzz"
    );
}

#[test]
fn sum_product_len_br() {
    assert_eq!(eval_int(r#"sum(1, 2, 3, 4)"#), 10);
    assert_eq!(eval_int(r#"product(2, 3, 7)"#), 42);
    assert_eq!(eval_int(r#"len(11, 22, 33)"#), 3);
}

#[test]
fn any_all_list_br() {
    assert_eq!(eval_int(r#"any { $_ > 0 } (-1, 0, 3)"#), 1);
    assert_eq!(eval_int(r#"all { $_ > 0 } (3, 4, 5)"#), 1);
    assert_eq!(eval_int(r#"all { $_ > 0 } (3, 0, 5)"#), 0);
}

#[test]
fn list_count_length_br() {
    assert_eq!(eval_int(r#"list_count(1, [2, 3], 4)"#), 4);
    assert_eq!(eval_int(r#"length("hello")"#), 5);
}

#[test]
fn sliding_pairs_adjacent_br() {
    assert_eq!(
        eval_string(r#"stringify(sliding_pairs(10, 20, 30))"#),
        "([10, 20], [20, 30])"
    );
}

#[test]
fn flatten_two_lists_br() {
    assert_eq!(
        eval_string(r#"join(",", flatten([1, 2], [30, 40]))"#),
        "1,2,30,40"
    );
}

#[test]
fn divmod_positive_br() {
    assert_eq!(eval_string(r#"join(",", divmod(17, 5))"#), "3,2");
}

#[test]
fn abs_sign_int_br() {
    assert_eq!(eval_int(r#"abs(-88)"#), 88);
    assert_eq!(eval_int(r#"sign(-9)"#), -1);
    assert_eq!(eval_int(r#"int(-3.9)"#), -3);
}

#[test]
fn merge_hash_nested_key_br() {
    assert_eq!(
        eval_int(
            r#"my %ha = (x => 1); my %hb = (x => 2, y => 3); my %m = %{ merge_hash(\%ha, \%hb) }; $m{x}"#
        ),
        2
    );
}
