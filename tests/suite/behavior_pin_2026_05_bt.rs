//! Behavior-pinning batch BT (2026-05-08): mesh, cartesian products, dedupe/power-set, list indexing
//! (`index_of` / `last_index_of` / `positions_of`), multiset ops, transforms (`rotate`, `swap_pairs`,
//! `repeat_list`, `intersperse_val`, `chunk_string`), block folds (`accumulate`, running aggregates),
//! angle conversion, roman helpers, string/encoding primitives, `coalesce`, JSON helpers, `strip_*`,
//! `safe_div`, `nth_root`.

use crate::common::*;

#[test]
fn mesh_two_lists_bt() {
    assert_eq!(
        eval_string(r#"stringify(mesh((1..3), ("a", "b")))"#),
        r#"(1, "a", 2, "b", 3, undef)"#
    );
}

#[test]
fn cartesian_product_bt() {
    assert_eq!(
        eval_string(r#"stringify(cartesian_product([1, 2], [10, 20]))"#),
        "([1, 10], [1, 20], [2, 10], [2, 20])"
    );
}

#[test]
fn partition_n_blocks_bt() {
    assert_eq!(
        eval_string(r#"stringify(partition_n(3, [1..10]))"#),
        "([1, 2, 3], [4, 5, 6], [7, 8, 9], [10])"
    );
}

#[test]
fn dedup_stable_neighbors_bt() {
    assert_eq!(
        eval_string(r#"stringify(dedup(1, 1, 2, 2, 9))"#),
        "(1, 2, 9)"
    );
}

#[test]
fn power_set_two_bt() {
    assert_eq!(
        eval_string(r#"stringify(power_set(1..2))"#),
        "([], [1], [2], [1, 2])"
    );
}

#[test]
fn index_by_key_strlen_bt() {
    assert_eq!(
        eval_string(r#"stringify(index_by sub { length($_[0]) }, ("bb", "z", "eee"))"#),
        r##"+{2 => "bb", 1 => "z", 3 => "eee"}"##
    );
}

#[test]
fn moving_average_window2_bt() {
    assert_eq!(
        eval_string(r#"stringify(moving_average(2, 10, 20, 30, 100))"#),
        "(15, 25, 65)"
    );
}

#[test]
fn exponential_moving_avg_bt() {
    assert_eq!(
        eval_string(r#"stringify(exponential_moving_average(0.5, 0, 10, 20, 30))"#),
        "(0, 5, 12.5, 21.25)"
    );
}

#[test]
fn cumsum_bt() {
    assert_eq!(
        eval_string(r#"stringify(cumsum(3, 7, -1, 4))"#),
        "(3, 10, 9, 13)"
    );
}

#[test]
fn cumprod_bt() {
    assert_eq!(
        eval_string(r#"stringify(cumprod(2, 3, 4, 5))"#),
        "(2, 6, 24, 120)"
    );
}

#[test]
fn accumulate_running_sum_bt() {
    assert_eq!(
        eval_string(r#"join(",", accumulate sub { $_[0] + $_[1] }, (100, -5, 6, -2))"#),
        "100,95,101,99"
    );
}

#[test]
fn running_max_tuple_bt() {
    assert_eq!(
        eval_string(r#"stringify(running_max(3, 1, 9, 8, 22))"#),
        "(3, 3, 9, 9, 22)"
    );
}

#[test]
fn multiset_intersection_pair_bt() {
    assert_eq!(
        eval_string(
            r#"my @m = multiset_intersection([1, 1, 2], [1, 2, 2]);
               join(",", sort { $a <=> $b } @m)"#
        ),
        "1,2"
    );
}

#[test]
fn rotate_left_two_bt() {
    assert_eq!(
        eval_string(r#"join(",", rotate(2, 1, 2, 3, 4, 5))"#),
        "3,4,5,1,2"
    );
}

#[test]
fn swap_pairs_flip_bt() {
    assert_eq!(
        eval_string(r#"join(",", swap_pairs(11, 12, 31, 41, 99))"#),
        "12,11,41,31,99"
    );
}

#[test]
fn repeat_list_cycles_bt() {
    assert_eq!(eval_int(r#"list_count(repeat_list(9, -1, 0, 2))"#), 27);
}

#[test]
fn radians_half_circle_bt() {
    assert_eq!(eval_string(r#"sprintf("%.6f", radians(180))"#), "3.141593");
}

#[test]
fn degrees_full_pi_bt() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", degrees(3.141592653589793))"#),
        "180.000000"
    );
}

#[test]
fn roman_roundtrip_bt() {
    assert_eq!(eval_string(r#"roman(1999)"#), "MCMXCIX");
    assert_eq!(eval_int(r#"roman_to_int("MCMXCIX")"#), 1999);
}

#[test]
fn pad_string_width_bt() {
    assert_eq!(eval_string(r#"pad_left("7", 4, "0")"#), "0007");
    assert_eq!(eval_string(r#"pad_right("ab", 5, ".")"#), "ab...");
}

#[test]
fn levenshtein_kitten_bt() {
    assert_eq!(eval_int(r#"levenshtein("kitten", "sitting")"#), 3);
}

#[test]
fn base_convert_hex_byte_bt() {
    assert_eq!(eval_int(r#"base_convert("ff", 16, 10)"#), 255);
}

#[test]
fn substring_overlap_count_bt() {
    assert_eq!(eval_int(r#"substring_count("abababa", "aba")"#), 2);
}

#[test]
fn index_last_positions_bt() {
    assert_eq!(eval_int(r#"index_of(2, -2, 2, 0, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"last_index_of(7, 7, 3, 7, 8, 7)"#), 4);
    assert_eq!(
        eval_string(r#"join(",", positions_of(9, 9, 1, 9, 9, 0, 9))"#),
        "0,2,3,5"
    );
}

#[test]
fn flip_args_tuple_bt() {
    assert_eq!(
        eval_string(r#"join(",", flip_args(100, 200, 300))"#),
        "300,200,100"
    );
}

#[test]
fn from_json_hash_field_bt() {
    assert_eq!(
        eval_string(
            r#"my %h = %{ from_json(q|{"u":[-2,88]}|) };
               join(",", @{$h{u}})"#
        ),
        "-2,88"
    );
}

#[test]
fn to_json_from_json_array_bt() {
    assert_eq!(
        eval_int(r#"my $a = from_json(to_json([11, 22])); int($a->[0] + $a->[1])"#),
        33
    );
}

#[test]
fn chunk_string_width_bt() {
    assert_eq!(
        eval_string(
            r#"my @c = chunk_string(3, "abcdefghij");
               join("|", @c)"#
        ),
        "abc|def|ghi|j"
    );
}

#[test]
fn intersperse_separator_bt() {
    assert_eq!(
        eval_string(r#"join(",", intersperse_val(-1, 10, 20, 30))"#),
        "10,-1,20,-1,30"
    );
}

#[test]
fn coalesce_first_truthy_bt() {
    assert_eq!(eval_int(r#"coalesce(undef, undef, 9, 88)"#), 9);
}

#[test]
fn strip_prefix_suffix_bt() {
    assert_eq!(eval_string(r#"strip_prefix("preX", "pre")"#), "X");
    assert_eq!(eval_string(r#"strip_suffix("Xtail", "tail")"#), "X");
}

#[test]
fn safe_div_bt() {
    assert_eq!(eval_string(r#"sprintf("%.4f", safe_div(10, 4))"#), "2.5000");
    assert_eq!(eval_int(r#"0 + defined(safe_div(9, 0))"#), 0);
}

#[test]
fn nth_root_cube_bt() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", nth_root(27, 3))"#),
        "3.000000"
    );
}
