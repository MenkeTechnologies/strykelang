//! Tree-interpreter extensions: `retry`, `rate_limit`, `every`, `gen` / `yield`.

use crate::common::*;

#[test]
fn retry_succeeds_first_attempt() {
    assert_eq!(eval_int(r#"retry { 42 } times => 3"#), 42);
}

#[test]
fn gen_yield_next_returns_value_and_more_flag() {
    assert_eq!(
        eval_string(
            r#"my $g = gen { yield 7; yield 8; };
            my $a = $g->next;
            my $b = $g->next;
            my $c = $g->next;
            ($a->[0], $a->[1], $b->[0], $b->[1], $c->[0], $c->[1]) |> join ','"#,
        ),
        "7,1,8,1,,0"
    );
}

#[test]
fn flat_map_alias_expands_array_results_like_map() {
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> flat_map { [$_ * 10, $_ + 1] } |> join ','"#),
        "10,2,20,3,30,4"
    );
}

#[test]
fn pflat_map_preserves_input_order_when_flattening() {
    let s = eval_string(r#"(3, 2, 1) |> pflat_map { [$_] } |> join ','"#);
    assert_eq!(s, "3,2,1");
}

#[test]
fn puniq_distinct_first_occurrence_order() {
    assert_eq!(
        eval_string(r#"(1, 2, 2, 1, 3, 1) |> puniq |> join ','"#),
        "1,2,3"
    );
    assert_eq!(eval_string(r#"(9, 8, 9) |> puniq |> join ','"#), "9,8");
}

#[test]
fn pfirst_returns_first_matching_value_in_order() {
    assert_eq!(eval(r#"pfirst { $_ > 2 } (1, 2, 5, 4, 3);"#).to_int(), 5);
}

#[test]
fn pfirst_empty_list_yields_undef_in_numeric_context() {
    assert_eq!(eval_int(r#"0 + (pfirst { $_ > 10 } ())"#), 0);
}

#[test]
fn pany_short_circuit_truth() {
    assert_eq!(eval_int(r#"pany { $_ == 5 } (1, 2, 3);"#), 0);
    assert_eq!(eval_int(r#"pany { $_ == 5 } (1, 2, 5, 3);"#), 1);
}

#[test]
fn bare_uniq_list_util_adjacent_dedup() {
    assert_eq!(eval_string(r#"(1, 1, 2, 3) |> uniq |> join ','"#), "1,2,3");
}

#[test]
fn bare_distinct_alias_matches_uniq() {
    assert_eq!(
        eval_string(r#"(1, 1, 2, 3) |> distinct |> join ','"#),
        "1,2,3"
    );
}

#[test]
fn reversed_alias_matches_reverse_list() {
    assert_eq!(eval_string(r#"(1, 2, 3) |> reversed |> join ','"#), "3,2,1");
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> reverse |> join ','"#),
        eval_string(r#"(1, 2, 3) |> reversed |> join ','"#)
    );
}

#[test]
fn flatten_one_level_peels_arrays_and_arefs() {
    assert_eq!(
        eval_string(r#"(1, [2, 3]) |> flatten |> join ','"#),
        "1,2,3"
    );
    assert_eq!(eval_int(r#"scalar flatten(1, [2, 3]);"#), 3);
}

#[test]
fn bare_shuffle_list_context_permutation() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @s = shuffle(7, 8, 9, 10);
            scalar @s;"#
        ),
        4
    );
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @in = (7, 8, 9, 10);
            my @s = shuffle @in;
            scalar @s;"#
        ),
        4
    );
    assert_eq!(
        eval_string(r#"(3, 1, 2, 2) |> shuffle |> sort { $a <=> $b } |> join '-'"#),
        "1-2-2-3"
    );
}

#[test]
fn bare_chunked_list_context_and_last_arg_is_size() {
    assert_eq!(eval_int(r#"scalar ((1, 2, 3, 4) |> chunked 2)"#), 2);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-3,4"
    );
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @in = (1, 2, 3, 4, 5);
            scalar (@in |> chunked 2);"#
        ),
        3
    );
    assert_eq!(
        eval_int(r#"scalar ((10, 20, 30) |> List::Util::chunked 2)"#),
        2
    );
}

#[test]
fn chunked_edge_cases_pipe_multi_array_and_n_zero() {
    assert_eq!(eval_int(r#"scalar ((1, 2, 3) |> chunked 0)"#), 0);
    assert_eq!(
        eval_int(r#"no strict 'vars'; my @e = (); scalar (@e |> chunked 4)"#),
        0
    );
    assert_eq!(eval_int(r#"scalar ((1, 2, 3) |> chunked 10)"#), 1);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> chunked 2 |> map { scalar @$_ } |> join "/""#),
        "2/2"
    );
    assert_eq!(
        eval_string(r#"(10, 20, 30) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "10,20-30"
    );
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2);
            my @b = (3, 4);
            scalar ((@a, @b) |> chunked 2);"#
        ),
        2
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-3"
    );
}

#[test]
fn windowed_sliding_pairs_like_example() {
    assert_eq!(eval_int(r#"scalar ((1, 2, 3) |> windowed 2)"#), 2);
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> windowed 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-2,3"
    );
    assert_eq!(
        eval_int(r#"scalar ((5, 6, 7) |> List::Util::windowed 2)"#),
        2
    );
}

#[test]
fn windowed_pipe_alternate_list_and_empty_array_operand() {
    assert_eq!(
        eval_string(r#"(9, 8, 7) |> windowed 2 |> map { join ",", @$_ } |> join "-""#),
        "9,8-8,7"
    );
    assert_eq!(
        eval_int(r#"no strict 'vars'; my @e = (); scalar (@e |> windowed 3)"#),
        0
    );
}

#[test]
fn windowed_no_partial_tail_empty_when_n_exceeds_len() {
    assert_eq!(eval_int(r#"scalar ((1, 2, 3) |> windowed 3)"#), 1);
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> windowed 3 |> map { join "-", @$_ } |> join ',' "#),
        "1-2-3"
    );
    assert_eq!(eval_int(r#"scalar ((1, 2) |> windowed 4)"#), 0);
    assert_eq!(
        eval_string(r#"(1, 2) |> windowed 4 |> map { join ",", @$_ } |> join "-""#),
        ""
    );
}

#[test]
fn windowed_zero_size_yields_no_windows() {
    assert_eq!(eval_int(r#"scalar ((9, 8) |> windowed 0)"#), 0);
    assert_eq!(
        eval_string(r#"(9, 8) |> windowed 0 |> map { join ",", @$_ } |> join "-""#),
        ""
    );
}

#[test]
fn windowed_unary_call_is_empty_list_until_piped() {
    assert_eq!(eval_int(r#"scalar windowed(2)"#), 0);
}

#[test]
fn chunked_unary_call_empty_without_list() {
    assert_eq!(eval_int(r#"scalar chunked(3)"#), 0);
}

#[test]
fn list_count_and_list_size_use_list_context_like_flatten_scalar() {
    assert_eq!(eval_int(r#"list_count(1, 2, 3)"#), 3);
    assert_eq!(eval_int(r#"list_size(10, 20)"#), 2);
    assert_eq!(eval_int(r#"list_count()"#), 0);
    assert_eq!(
        eval_int(r#"no strict 'vars'; my @a = (5, 6, 7); list_count(@a, 8)"#),
        4
    );
    assert_eq!(eval_int(r#"scalar flatten(1, 2, [3, 4])"#), 4);
    assert_eq!(eval_int(r#"list_count(1, 2, [3, 4])"#), 4);
    assert_eq!(eval_int(r#"(1, 2, 3) |> list_count"#), 3);
    assert_eq!(eval_int(r#"(1, 2, 3) |> count"#), 3);
    assert_eq!(eval_int(r#"(1, 2) |> size"#), 2);
    assert_eq!(eval_int(r#"list_count("tom")"#), 1);
    assert_eq!(eval_int(r#""tom" |> cnt"#), 3);
    assert_eq!(eval_int(r#""tom" |> size"#), 3);
    assert_eq!(eval_int(r#""tom" |> count"#), 3);
    assert_eq!(eval_int(r#"1..5 |> cnt"#), 5);
    assert_eq!(eval_int(r#"1..5 |> size"#), 5);
    assert_eq!(eval_int(r#"1..5 |> count"#), 5);
    assert_eq!(eval_int(r#"cnt("x", "y")"#), 2);
}

#[test]
fn ruby_aliases_inject_detect_find_find_all() {
    assert_eq!(eval_int(r#"inject { $a + $b } (1, 2, 3, 4)"#), 10);
    assert_eq!(eval_int(r#"detect { $_ > 2 } (1, 2, 5, 4)"#), 5);
    assert_eq!(eval_int(r#"find { $_ > 2 } (1, 2, 5, 4)"#), 5);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> find_all { $_ % 2 == 0 } |> join ','"#),
        "2,4"
    );
}

#[test]
fn take_while_drop_while_and_with_index_pairs() {
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4, 1) |> take_while { $_ < 4 } |> join ','"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4, 1) |> drop_while { $_ < 4 } |> join ','"#),
        "4,1"
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> with_index |> map { join ",", @$_ } |> join '/'"#),
        "1,0/2,1/3,2"
    );
    assert_eq!(
        eval_string(r#"(10, 20) |> with_index |> map { join ",", @$_ } |> join '/'"#),
        "10,0/20,1"
    );
    assert_eq!(eval_int(r#"scalar with_index((7, 8, 9))"#), 3);
    assert_eq!(eval_int(r#"scalar take_while { 1 } (5, 6)"#), 2);
    assert_eq!(eval_int(r#"scalar drop_while { $_ < 0 } (1, 2)"#), 2);
}

#[test]
fn tap_peek_pass_through_and_pipe() {
    assert_eq!(
        eval_string(r#"join ',', tap { 1 } (10, 20, 30)"#),
        "10,20,30"
    );
    assert_eq!(
        eval_string(r#"join ',', peek { 1 } (10, 20, 30)"#),
        "10,20,30"
    );
    assert_eq!(
        eval_string(r#"(7, 8, 9) |> tap { 1 } |> join ','"#),
        "7,8,9"
    );
    assert_eq!(eval_int(r#"scalar tap { 1 } (1, 2, 3)"#), 3);
    assert_eq!(
        eval_string(
            r#"join ',', pipeline(1, 2, 3)->peek(sub { 1 })->map(sub { $_ * 2 })->collect()"#
        ),
        "2,4,6"
    );
}

#[test]
fn list_fold_same_semantics_as_reduce_and_pipe() {
    assert_eq!(eval_int(r#"(1, 2, 3, 4) |> fold { $a + $b }"#), 10);
    assert_eq!(eval_int(r#"(1, 2, 3, 4) |> reduce { $a + $b }"#), 10);
    assert_eq!(
        eval_string(r#"qw(a b c) |> List::Util::fold { $a . $b }"#),
        "abc"
    );
    assert_eq!(eval_int(r#"(2, 3, 4) |> fold { $a * $b }"#), 24);
    assert_eq!(eval_int(r#"(42) |> fold { $a + $b }"#), 42);
}

#[test]
fn fold_reduce_undef_on_empty_list_and_fold_max() {
    assert_eq!(eval_int(r#"defined(() |> reduce { $a + $b }) ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"defined(() |> fold { $a + $b }) ? 1 : 0"#), 0);
    assert_eq!(
        eval_int(r#"(3, 7, 1, 9, 2) |> fold { $a > $b ? $a : $b }"#),
        9
    );
    assert_eq!(
        eval_int(r#"(3, 7, 1, 9, 2) |> reduce { $a > $b ? $a : $b }"#),
        9
    );
}

#[test]
fn chunked_windowed_reject_legacy_multi_arg_at_parse() {
    assert!(matches!(
        parse_err_kind("chunked(1, 2, 3, 4, 2);"),
        perlrs::error::ErrorKind::Syntax
    ));
    assert!(matches!(
        parse_err_kind("windowed(1, 2, 3, 2);"),
        perlrs::error::ErrorKind::Syntax
    ));
}

#[test]
fn readme_chunked_windowed_join_shapes() {
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-3,4"
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> windowed 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-2,3"
    );
    assert_eq!(
        eval_string(r#"(10, 20, 30) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "10,20-30"
    );
}

#[test]
fn bare_any_all_none_list_util_semantics() {
    assert_eq!(eval_int(r#"any { $_ > 2 } (1, 2, 3);"#), 1);
    assert_eq!(eval_int(r#"any { $_ > 5 } (1, 2, 3);"#), 0);
    assert_eq!(eval_int(r#"all { $_ > 0 } (1, 2, 3);"#), 1);
    assert_eq!(eval_int(r#"all { $_ < 3 } (1, 2, 3);"#), 0);
    assert_eq!(eval_int(r#"none { $_ < 0 } (1, 2, 3);"#), 1);
    assert_eq!(eval_int(r#"none { $_ > 0 } (1, 2, 3);"#), 0);
}

#[test]
fn zip_evaluates_array_operands_in_list_context() {
    assert_eq!(
        eval_int(
            r#"my @a = (1, 2);
            my @b = (10, 20);
            my @z = zip(@a, @b);
            $z[0]->[0] + $z[0]->[1];"#,
        ),
        11
    );
}

#[test]
fn chunk_by_and_group_by_split_consecutive_runs_by_key() {
    assert_eq!(
        eval_string(
            r#"(1, 3, 2, 4, 5) |> chunk_by { $_ % 2 } |> map { join ",", @$_ } |> join '/'"#,
        ),
        "1,3/2,4/5"
    );
    assert_eq!(
        eval_string(
            r#"(1, 3, 2, 4, 5) |> group_by { $_ % 2 } |> map { join ",", @$_ } |> join '/'"#
        ),
        "1,3/2,4/5"
    );
    assert_eq!(
        eval_string(
            r#"(1, 3, 2, 4, 5) |> group_by $_ % 2, () |> map { join ",", @$_ } |> join '/'"#
        ),
        "1,3/2,4/5"
    );
    assert_eq!(eval_int(r#"scalar chunk_by { $_ } (1, 2, 3)"#), 3);
    assert_eq!(eval_int(r#"scalar chunk_by { 0 } (1, 2, 3)"#), 1);
}

/// Every list-oriented builtin that participates in `|>` special-casing should accept a piped LHS.
#[test]
fn new_list_functions_all_support_pipe_forward() {
    assert_eq!(eval_string(r#"(1, 1, 2) |> uniq |> join ','"#), "1,2");
    assert_eq!(eval_string(r#"(3, 3, 4) |> distinct |> join ','"#), "3,4");
    assert_eq!(
        eval_string(r#"(1, [2, 3]) |> flatten |> join ','"#),
        "1,2,3"
    );
    assert_eq!(eval_int(r#"(5, 6, 7) |> list_count"#), 3);
    assert_eq!(eval_int(r#"(9, 8) |> list_size"#), 2);
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> with_index |> map { join ",", @$_ } |> join "/""#),
        "1,0/2,1/3,2"
    );
    assert_eq!(
        eval_string(r#"(4, 1, 3) |> shuffle |> sort { $a <=> $b } |> join "-""#),
        "1-3-4"
    );
    assert_eq!(eval_int(r#"(1, 2, 5) |> any { $_ == 5 }"#), 1);
    assert_eq!(eval_int(r#"(1, 2, 3) |> all { $_ > 0 }"#), 1);
    assert_eq!(eval_int(r#"(1, 2, 3) |> none { $_ < 0 }"#), 1);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4, 1) |> take_while { $_ < 4 } |> join ','"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4, 1) |> drop_while { $_ < 4 } |> join ','"#),
        "4,1"
    );
    assert_eq!(eval_int(r#"(1, 2, 5, 4) |> pfirst { $_ > 3 }"#), 5);
    assert_eq!(eval_int(r#"(1, 2, 6) |> pany { $_ == 6 }"#), 1);
    assert_eq!(eval_string(r#"(1, 1, 2) |> puniq |> join ','"#), "1,2");
    assert_eq!(
        eval_string(r#"(10, 20, 30) |> take 2 |> join ','"#),
        "10,20"
    );
    assert_eq!(
        eval_string(r#"(10, 20, 30) |> head 2 |> join ','"#),
        "10,20"
    );
    assert_eq!(
        eval_string(r#"(10, 20, 30) |> tail 2 |> join ','"#),
        "20,30"
    );
    assert_eq!(
        eval_string(r#"(10, 20, 30) |> drop 1 |> join ','"#),
        "20,30"
    );
    assert_eq!(eval_string(r#"(1, 2, 3) |> reversed |> join ','"#), "3,2,1");
    // `[1,2] |> zip [10,20]` desugars to `zip([1,2], [10,20])`, which yields two
    // row arrayrefs `[1,10]`, `[2,20]`. `list_count` then peels one level of
    // arrayrefs, flattening 2 rows × 2 cols into 4 scalar elements. This is the
    // left-associative behaviour — an earlier version of this test asserted `2`,
    // which only worked because `|>` was silently right-associative and the
    // chain parsed as `zip([1,2], list_count([10,20]))`, bypassing `zip`.
    assert_eq!(eval_int(r#"[1, 2] |> zip [10, 20] |> list_count"#), 4);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-3,4"
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> windowed 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-2,3"
    );
    assert_eq!(
        eval_string(
            r#"(1, 3, 2, 4, 5) |> chunk_by { $_ % 2 } |> map { join ",", @$_ } |> join "/""#,
        ),
        "1,3/2,4/5"
    );
    assert_eq!(
        eval_string(
            r#"(1, 3, 2, 4, 5) |> group_by $_ % 2, () |> map { join ",", @$_ } |> join "/""#,
        ),
        "1,3/2,4/5"
    );
    assert_eq!(eval_int(r#"(1, 2, 3, 4) |> fold { $a + $b }"#), 10);
    assert_eq!(eval_int(r#"(1, 2, 3, 4) |> reduce { $a + $b }"#), 10);
    // Bare List::Util-style aggregates: slurpy `@_` / list expr must flatten under `|>`.
    assert_eq!(eval_int(r#"(1, 2, 3) |> sum"#), 6);
    assert_eq!(eval_int(r#"() |> sum0"#), 0);
    assert_eq!(eval_int(r#"(2, 3, 4) |> product"#), 24);
    assert_eq!(eval_int(r#"(3, 9, 2) |> min"#), 2);
    assert_eq!(eval_int(r#"(3, 9, 2) |> max"#), 9);
    assert_eq!(eval_int(r#"(2, 4, 6) |> mean"#), 4);
    assert_eq!(eval_string(r#"qw(b a) |> List::Util::maxstr"#), "b");
}
