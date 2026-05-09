//! Behavior-pinning batch BU (2026-05-08): bounds & search (`lower_bound`, `upper_bound`,
//! `binary_search`, `linear_search`), ranking (`rank`, `dense_rank`, `percentile_rank`, `quartiles`),
//! multiset union/difference (sorted pins), run-length decode, range predicates, `hash_zip`, codecs
//! (`url_encode`/`url_decode`, `to_hex`/`from_hex`, `json_decode`), stats (`weighted_mean`, `z_score`,
//! `span`, `running_min`, `distinct_count`, `mode`), angle-ish smoothing (`lerp`, `smoothstep`),
//! string/list tools (`longest`, `shortest`, `lines`, `to_csv_line`, `format_bytes`, `format_duration`,
//! `rgb_to_hex`, `take_every`), numeric (`spaceship`, `is_multiple_of`, `tax_amount`, `approx_eq`,
//! `signum`, `gcd`/`lcm`, `ceil`/`floor`/`round`, `range`, `product`, `abs`, `copysign`, `hypot`),
//! `to_json` object wire shape.

use crate::common::*;

#[test]
fn bound_binary_search_bu() {
    assert_eq!(eval_int(r#"lower_bound(5, [1, 3, 5, 7, 9])"#), 2);
    assert_eq!(eval_int(r#"upper_bound(5, [1, 3, 5, 5, 7])"#), 4);
    assert_eq!(eval_int(r#"binary_search(7, [1, 3, 5, 7, 9, 11])"#), 3);
    assert_eq!(eval_int(r#"binary_search(6, [1, 3, 5, 7, 9])"#), -1);
}

#[test]
fn rank_and_dense_bu() {
    assert_eq!(
        eval_string(r#"stringify(rank(30, 10, 30, 5))"#),
        "(3, 2, 4, 1)"
    );
    assert_eq!(
        eval_string(r#"stringify(dense_rank(30, 10, 30, 5))"#),
        "(3, 2, 3, 1)"
    );
}

#[test]
fn percentile_rank_quarter_bu() {
    assert_eq!(
        eval_string(r#"sprintf("%.10f", percentile_rank(25, [10, 20, 30, 40, 50]))"#),
        "40.0000000000"
    );
}

#[test]
fn quartiles_small_bu() {
    assert_eq!(
        eval_string(r#"stringify(quartiles(1, 2, 3, 4, 5, 6, 7, 8))"#),
        "(3, 5, 7)"
    );
}

#[test]
fn multiset_union_sorted_bu() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } multiset_union([4, 4], [1, 2]))"#),
        "1,2,4,4"
    );
}

#[test]
fn rld_decode_bu() {
    assert_eq!(eval_string(r#"join("", rld(["x", 3], ["y", 2]))"#), "xxxyy");
}

#[test]
fn consecutive_eq_runs_bu() {
    assert_eq!(eval_int(r#"consecutive_eq(1, 1, 1)"#), 1);
    assert_eq!(eval_int(r#"consecutive_eq(1, 1, 2)"#), 0);
}

#[test]
fn between_and_half_open_bu() {
    assert_eq!(eval_int(r#"is_between(5, 1, 10)"#), 1);
    assert_eq!(eval_int(r#"is_in_range(5, 5, 10)"#), 1);
    assert_eq!(eval_int(r#"is_in_range(10, 5, 10)"#), 0);
}

#[test]
fn hash_zip_pairs_bu() {
    assert_eq!(
        eval_string(r#"stringify(hash_zip([1, 2], [10, 20]))"#),
        "+{1 => 10, 2 => 20}"
    );
}

#[test]
fn url_encode_decode_bu() {
    assert_eq!(eval_string(r#"url_encode("a b")"#), "a%20b");
    assert_eq!(eval_string(r#"url_decode("a%20b")"#), "a b");
}

#[test]
fn hex_from_to_bu() {
    assert_eq!(eval_string(r#"to_hex(255)"#), "ff");
    assert_eq!(eval_int(r#"int(from_hex("ff"))"#), 255);
}

#[test]
fn weighted_mean_bu() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", weighted_mean([10, 20, 30], [1, 2, 3]))"#),
        "23.3333"
    );
}

#[test]
fn z_score_unit_bu() {
    assert_eq!(
        eval_string(r#"sprintf("%.6f", z_score(110, 100, 10))"#),
        "1.000000"
    );
}

#[test]
fn span_list_bu() {
    assert_eq!(eval_int(r#"int(span(1, 10, 4))"#), 9);
}

#[test]
fn distinct_count_bu() {
    assert_eq!(eval_int(r#"distinct_count(1, 2, 2, 3, 9, 3)"#), 4);
}

#[test]
fn longest_shortest_bu() {
    assert_eq!(eval_string(r#"longest("a", "bb", "ccc")"#), "ccc");
    assert_eq!(eval_string(r#"shortest("a", "bb", "ccc")"#), "a");
}

#[test]
fn format_bytes_kb_bu() {
    assert_eq!(eval_string(r#"format_bytes(1536)"#), "1.50 KB");
}

#[test]
fn take_every_stride_bu() {
    assert_eq!(
        eval_string(r#"join(",", take_every(2, 10, 11, 12, 13, 14))"#),
        "10,12,14"
    );
}

#[test]
fn to_csv_line_bu() {
    assert_eq!(eval_string(r#"to_csv_line(1, "a,b", 3)"#), r#"1,"a,b",3"#);
}

#[test]
fn lines_split_bu() {
    assert_eq!(eval_string(r#"join("|", lines("p\nq\nr"))"#), "p|q|r");
}

#[test]
fn is_multiple_of_bu() {
    assert_eq!(eval_int(r#"is_multiple_of(100, 25)"#), 1);
    assert_eq!(eval_int(r#"is_multiple_of(100, 33)"#), 0);
}

#[test]
fn rgb_to_hex_bu() {
    assert_eq!(eval_string(r#"rgb_to_hex(255, 128, 64)"#), "#ff8040");
}

#[test]
fn running_min_tuple_bu() {
    assert_eq!(
        eval_string(r#"stringify(running_min(5, 3, 7, 2, 9))"#),
        "(5, 3, 3, 2, 2)"
    );
}

#[test]
fn tax_amount_bu() {
    assert_eq!(
        eval_string(r#"sprintf("%.2f", tax_amount(200, 10))"#),
        "20.00"
    );
}

#[test]
fn spaceship_compare_bu() {
    assert_eq!(eval_int(r#"(9 <=> 11)"#), -1);
    assert_eq!(eval_int(r#"(11 <=> 9)"#), 1);
    assert_eq!(eval_int(r#"(7 <=> 7)"#), 0);
}

#[test]
fn linear_search_bu() {
    assert_eq!(eval_int(r#"linear_search(2, 10, 20, 30, 2, 99)"#), 3);
}

#[test]
fn multiset_difference_sorted_bu() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } multiset_difference([1, 1, 2, 3], [1, 2]))"#),
        "1,3"
    );
}

#[test]
fn lerp_smoothstep_bu() {
    assert_eq!(eval_int(r#"int(lerp(0, 100, 0.25))"#), 25);
    assert_eq!(
        eval_string(r#"sprintf("%.6f", smoothstep(0, 10, 5))"#),
        "0.500000"
    );
}

#[test]
fn json_decode_array_bu() {
    assert_eq!(
        eval_string(r#"stringify(json_decode(q|[1,2,3]|))"#),
        "(1, 2, 3)"
    );
}

#[test]
fn copysign_hypot_bu() {
    assert_eq!(
        eval_string(r#"sprintf("%.4f", copysign(10, -1))"#),
        "-10.0000"
    );
    assert_eq!(eval_string(r#"sprintf("%.4f", hypot(3, 4))"#), "5.0000");
}

#[test]
fn format_duration_bu() {
    assert_eq!(eval_string(r#"format_duration(3665)"#), "1h 1m 5s");
}

#[test]
fn signum_scalar_bu() {
    assert_eq!(eval_int(r#"signum(-25)"#), -1);
    assert_eq!(eval_int(r#"signum(3)"#), 1);
}

#[test]
fn approx_eq_tol_bu() {
    assert_eq!(eval_int(r#"approx_eq(1.0, 1.0001, 0.01)"#), 1);
    assert_eq!(eval_int(r#"approx_eq(1.0, 2.0, 0.01)"#), 0);
}

#[test]
fn gcd_lcm_pair_bu() {
    assert_eq!(eval_int(r#"gcd(81, 135)"#), 27);
    assert_eq!(eval_int(r#"lcm(14, 21)"#), 42);
}

#[test]
fn mode_scalar_bu() {
    assert_eq!(eval_int(r#"mode(1, 2, 2, 9)"#), 2);
}

#[test]
fn range_inclusive_list_bu() {
    assert_eq!(eval_string(r#"join(",", range(3, 7))"#), "3,4,5,6,7");
}

#[test]
fn ceil_floor_round_bu() {
    assert_eq!(eval_int(r#"int(ceil(2.001))"#), 3);
    assert_eq!(eval_int(r#"int(floor(-2.001))"#), -3);
    assert_eq!(eval_int(r#"int(round(2.6))"#), 3);
    assert_eq!(eval_int(r#"int(round(-2.6))"#), -3);
}

#[test]
fn product_small_chain_bu() {
    assert_eq!(eval_int(r#"product(2, 3, 4, 5)"#), 120);
}

#[test]
fn to_json_hash_shape_bu() {
    assert_eq!(
        eval_string(r#"to_json({b => 1, a => 2})"#),
        r#"{"b":1,"a":2}"#
    );
}

#[test]
fn abs_signed_bu() {
    assert_eq!(eval_int(r#"abs(-44)"#), 44);
    assert_eq!(eval_int(r#"abs(7)"#), 7);
}
