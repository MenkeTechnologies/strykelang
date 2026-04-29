//! Language extensions: `retry`, `rate_limit`, `every`, `gen` / `yield`.

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
fn maps_streams_through_pipe_uc_comma_form() {
    assert_eq!(eval_string(r#"(qw(a b)) |> maps uc |> join ','"#), "A,B");
}

#[test]
fn filter_streams_while_grep_stays_eager() {
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> filter { $_ % 2 == 0 } |> join ','"#),
        "2,4"
    );
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> grep { $_ % 2 == 0 } |> join ','"#),
        "2,4"
    );
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $root = tempdir();
            open my $f, '>', "$root/y" or die;
            close $f;
            fr($root) |> filter { length $_ == 1 } |> join ",""#,
        ),
        "y"
    );
}

#[test]
fn maps_streams_iterator_from_fr_shape_without_double_read() {
    // `fr` yields a pull iterator; `maps` must not materialize it up front.
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $root = tempdir();
            open my $f, '>', "$root/x" or die;
            close $f;
            fr($root) |> maps { uc $_ } |> join ",""#,
        ),
        "X"
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
    assert_eq!(eval(r#"pfirst { $_ > 2 } (1, 2, 5, 4, 3)"#).to_int(), 5);
}

#[test]
fn pfirst_empty_list_yields_undef_in_numeric_context() {
    assert_eq!(eval_int(r#"0 + (pfirst { $_ > 10 } ())"#), 0);
}

#[test]
fn pany_short_circuit_truth() {
    assert_eq!(eval_int(r#"pany { $_ == 5 } (1, 2, 3)"#), 0);
    assert_eq!(eval_int(r#"pany { $_ == 5 } (1, 2, 5, 3)"#), 1);
}

#[test]
fn bare_uniq_adjacent_dedup() {
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
        eval_string(r#"(1, 2, 3) |> rev |> join ','"#),
        eval_string(r#"(1, 2, 3) |> reversed |> join ','"#)
    );
}

#[test]
fn flatten_one_level_peels_arrays_and_arefs() {
    assert_eq!(
        eval_string(r#"(1, [2, 3]) |> flatten |> join ','"#),
        "1,2,3"
    );
    assert_eq!(eval_int(r#"list_count(flatten(1, [2, 3]))"#), 3);
}

#[test]
fn bare_shuffle_list_context_permutation() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @s = shuffle(7, 8, 9, 10);
            len(@s)"#
        ),
        4
    );
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @in = (7, 8, 9, 10);
            my @s = shuffle @in;
            len(@s)"#
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
    assert_eq!(eval_int(r#"my @r = (1, 2, 3, 4) |> chunked 2; len(@r)"#), 2);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> chunked 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-3,4"
    );
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @in = (1, 2, 3, 4, 5);
            my @r = @in |> chunked 2;
            len(@r)"#
        ),
        3
    );
    assert_eq!(eval_int(r#"my @r = (10, 20, 30) |> chunked 2; len(@r)"#), 2);
}

#[test]
fn chunked_edge_cases_pipe_multi_array_and_n_zero() {
    assert_eq!(eval_int(r#"my @r = (1, 2, 3) |> chunked 0; len(@r)"#), 0);
    assert_eq!(
        eval_int(r#"no strict 'vars'; my @e = (); my @r = @e |> chunked 4; len(@r)"#),
        0
    );
    assert_eq!(eval_int(r#"my @r = (1, 2, 3) |> chunked 10; len(@r)"#), 1);
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> chunked 2 |> map { len(@$_) } |> join "/""#),
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
            my @r = (@a, @b) |> chunked 2;
            len(@r)"#
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
    assert_eq!(eval_int(r#"my @r = (1, 2, 3) |> windowed 2; len(@r)"#), 2);
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> windowed 2 |> map { join ",", @$_ } |> join "-""#),
        "1,2-2,3"
    );
    assert_eq!(eval_int(r#"my @r = (5, 6, 7) |> windowed 2; len(@r)"#), 2);
}

#[test]
fn windowed_pipe_alternate_list_and_empty_array_operand() {
    assert_eq!(
        eval_string(r#"(9, 8, 7) |> windowed 2 |> map { join ",", @$_ } |> join "-""#),
        "9,8-8,7"
    );
    assert_eq!(
        eval_int(r#"no strict 'vars'; my @e = (); my @r = @e |> windowed 3; len(@r)"#),
        0
    );
}

#[test]
fn windowed_no_partial_tail_empty_when_n_exceeds_len() {
    assert_eq!(eval_int(r#"my @r = (1, 2, 3) |> windowed 3; len(@r)"#), 1);
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> windowed 3 |> map { join "-", @$_ } |> join ',' "#),
        "1-2-3"
    );
    assert_eq!(eval_int(r#"my @r = (1, 2) |> windowed 4; len(@r)"#), 0);
    assert_eq!(
        eval_string(r#"(1, 2) |> windowed 4 |> map { join ",", @$_ } |> join "-""#),
        ""
    );
}

#[test]
fn windowed_zero_size_yields_no_windows() {
    assert_eq!(eval_int(r#"my @r = (9, 8) |> windowed 0; len(@r)"#), 0);
    assert_eq!(
        eval_string(r#"(9, 8) |> windowed 0 |> map { join ",", @$_ } |> join "-""#),
        ""
    );
}

#[test]
fn windowed_unary_call_is_empty_list_until_piped() {
    assert_eq!(eval_int(r#"list_count(windowed(2))"#), 0);
}

#[test]
fn chunked_unary_call_empty_without_list() {
    assert_eq!(eval_int(r#"list_count(chunked(3))"#), 0);
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
    assert_eq!(eval_int(r#"list_count(flatten(1, 2, [3, 4]))"#), 4);
    assert_eq!(eval_int(r#"list_count(1, 2, [3, 4])"#), 4);
    assert_eq!(eval_int(r#"(1, 2, 3) |> list_count"#), 3);
    assert_eq!(eval_int(r#"(1, 2, 3) |> count"#), 3);
    assert_eq!(eval_int(r#"list_count("tom")"#), 1);
    assert_eq!(eval_int(r#""tom" |> cnt"#), 3);
    assert_eq!(eval_int(r#""tom" |> count"#), 3);
    assert_eq!(eval_int(r#"1..5 |> cnt"#), 5);
    assert_eq!(eval_int(r#"1..5 |> count"#), 5);
    assert_eq!(eval_int(r#"cnt("x", "y")"#), 2);
}

#[test]
fn size_returns_file_byte_size_like_dash_s() {
    // size($path) — explicit arg
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $root = tempdir();
            my $p = "$root/sz";
            open my $f, '>', $p or die;
            print $f "hello!";
            close $f;
            size($p)"#,
        ),
        6,
    );
    // size — no args, defaults to $_
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $root = tempdir();
            my $p = "$root/sz2";
            open my $f, '>', $p or die;
            print $f "12";
            close $f;
            $_ = $p;
            size"#,
        ),
        2,
    );
    // Pipeline + map — each file wrapped as `{path => size}` hashref.
    // `$_` inside the map is the full path that was piped in, so the JSON
    // key is the whole path (e.g. `/var/folders/.../sz3`) — we only assert
    // on the tail so the test isn't tied to the tmpdir layout.
    let out = eval_string(
        r#"no strict 'vars';
        my $root = tempdir();
        my $p = "$root/sz3";
        open my $f, '>', $p or die;
        print $f "abcd";
        close $f;
        ($p) |> map +{ $_ => size } |> to_json"#,
    );
    assert!(
        out.contains(r#"sz3":4"#),
        "expected key ending in `sz3\":4` (path → size) in {out}",
    );
    // Nonexistent path → undef, not 0 (matches `-s` semantics)
    assert_eq!(
        eval_int(r#"defined(size("this/path/should/never/exist")) ? 1 : 0"#),
        0,
    );
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
    assert_eq!(eval_int(r#"my @r = with_index((7, 8, 9)); len(@r)"#), 3);
    assert_eq!(eval_int(r#"my @r = take_while { 1 } (5, 6); len(@r)"#), 2);
    assert_eq!(
        eval_int(r#"my @r = drop_while { $_ < 0 } (1, 2); len(@r)"#),
        2
    );
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
    assert_eq!(eval_int(r#"list_count(tap { 1 } (1, 2, 3))"#), 3);
    assert_eq!(
        eval_string(
            r#"join ',', pipeline(1, 2, 3)->peek(fn { 1 })->map(fn { $_ * 2 })->collect()"#
        ),
        "2,4,6"
    );
}

#[test]
fn list_fold_same_semantics_as_reduce_and_pipe() {
    assert_eq!(eval_int(r#"(1, 2, 3, 4) |> fold { $a + $b }"#), 10);
    assert_eq!(eval_int(r#"(1, 2, 3, 4) |> reduce { $a + $b }"#), 10);
    assert_eq!(eval_string(r#"qw(a b c) |> fold { $a . $b }"#), "abc");
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
        parse_err_kind("chunked(1, 2, 3, 4, 2)"),
        stryke::error::ErrorKind::Syntax
    ));
    assert!(matches!(
        parse_err_kind("windowed(1, 2, 3, 2)"),
        stryke::error::ErrorKind::Syntax
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
fn bare_any_all_none_semantics() {
    assert_eq!(eval_int(r#"any { $_ > 2 } (1, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"any { $_ > 5 } (1, 2, 3)"#), 0);
    assert_eq!(eval_int(r#"all { $_ > 0 } (1, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"all { $_ < 3 } (1, 2, 3)"#), 0);
    assert_eq!(eval_int(r#"none { $_ < 0 } (1, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"none { $_ > 0 } (1, 2, 3)"#), 0);
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
    assert_eq!(eval_int(r#"my @r = chunk_by { $_ } (1, 2, 3); len(@r)"#), 3);
    assert_eq!(eval_int(r#"my @r = chunk_by { 0 } (1, 2, 3); len(@r)"#), 1);
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
    // Bare list-builtin aggregates: slurpy `@_` / list expr must flatten under `|>`.
    assert_eq!(eval_int(r#"(1, 2, 3) |> sum"#), 6);
    assert_eq!(eval_int(r#"() |> sum0"#), 0);
    assert_eq!(eval_int(r#"(2, 3, 4) |> product"#), 24);
    assert_eq!(eval_int(r#"(3, 9, 2) |> min"#), 2);
    assert_eq!(eval_int(r#"(3, 9, 2) |> max"#), 9);
    assert_eq!(eval_int(r#"(2, 4, 6) |> mean"#), 4);
    assert_eq!(eval_string(r#"qw(b a) |> maxstr"#), "b");
}

#[test]
fn pipe_forward_method_call() {
    // Basic: pipe into method call
    assert_eq!(
        eval_string(
            r#"package Fmt;
fn new { bless {}, $_[0] }
fn exclaim { $_[1] . "!" }
package main;
my $f = Fmt->new;
my $r = "hello" |> $f->exclaim;
$r"#,
        ),
        "hello!"
    );
    // Chained: pipe through multiple method calls
    assert_eq!(
        eval_string(
            r#"package Str;
fn new { bless {}, $_[0] }
fn upper { uc $_[1] }
fn wrap { my ($self, $s, $ch) = @_; $ch . $s . $ch }
package main;
my $s = Str->new;
"hello" |> $s->upper |> $s->wrap("*")"#,
        ),
        "*HELLO*"
    );
    // Mixed: builtin then method
    assert_eq!(
        eval_string(
            r#"package Fmt;
fn new { bless {}, $_[0] }
fn exclaim { $_[1] . "!" }
package main;
my $f = Fmt->new;
"hello" |> uc |> $f->exclaim"#,
        ),
        "HELLO!"
    );
}

// ── $_0, $_1, $_N closure arguments ──

#[test]
fn closure_args_in_named_sub_single_arg() {
    assert_eq!(eval_int(r#"fn dbl { $_0 * 2 } dbl(21)"#), 42);
}

#[test]
fn closure_args_in_named_sub_two_args() {
    assert_eq!(eval_int(r#"fn add { $_0 + $_1 } add(3, 4)"#), 7);
}

#[test]
fn closure_args_in_named_sub_three_args() {
    assert_eq!(eval_int(r#"fn mul3 { $_0 * $_1 * $_2 } mul3(2, 3, 4)"#), 24);
}

#[test]
fn closure_args_in_anonymous_fn() {
    assert_eq!(
        eval_int(r#"my $f = fn { $_0 + $_1 + $_2 }; $f->(1, 2, 3)"#),
        6
    );
}

#[test]
fn closure_args_in_thread_with_named_subs() {
    assert_eq!(
        eval_int(r#"fn dbl { $_0 * 2 } fn add10 { $_0 + 10 } thread 5 dbl add10"#),
        20
    );
}

#[test]
fn closure_args_in_thread_chain_of_udfs() {
    assert_eq!(
        eval_int(
            r#"fn dbl { $_0 * 2 }
               fn tripl { $_0 * 3 }
               fn add5 { $_0 + 5 }
               thread 2 dbl tripl add5"#
        ),
        17
    );
}

#[test]
fn thread_udf_with_explicit_paren_args() {
    // `t VAL func($_-bearing args)` binds VAL to `$_` and evaluates the call,
    // so the threaded value can be placed at any argument position.
    // First-arg form
    assert_eq!(
        eval_int(r#"fn add2 { $_0 + $_1 } thread 10 add2($_, 5)"#),
        15
    );
    // Last-arg form (impossible with implicit first-arg injection)
    assert_eq!(
        eval_int(r#"fn sub2 { $_0 - $_1 } thread 10 sub2(20, $_)"#),
        10
    );
    // Middle-arg form
    assert_eq!(
        eval_int(r#"fn add3 { $_0 + $_1 + $_2 } thread 10 add3(5, $_, 10)"#),
        25
    );
    // Chained calls — output of one stage becomes the `$_` for the next
    assert_eq!(
        eval_int(r#"fn add2 { $_0 + $_1 } thread 10 add2($_, 5) add2($_, 100)"#),
        115
    );
    // Mixes with bare-function stages
    assert_eq!(
        eval_int(r#"fn sub2 { $_0 - $_1 } thread 10 sub2($_, 15) abs"#),
        5
    );
    // `$_` inside a nested expression
    assert_eq!(
        eval_int(r#"fn mul { $_0 * $_1 } thread 10 mul($_ + 1, 2)"#),
        22
    );
    // `$_` inside a nested unary builtin
    assert_eq!(
        eval_int(r#"fn add2 { $_0 + $_1 } thread 10 add2(abs($_), 5)"#),
        15
    );
    // `$_` inside an interpolated string
    assert_eq!(
        eval_string(r#"fn greet { "$_0 from $_1" } thread "alice" greet("hi $_", "bob")"#),
        "hi alice from bob"
    );
}

#[test]
fn closure_args_in_pipe_map() {
    assert_eq!(
        eval_string(r#"(1..5) |> map { $_0 * 2 } |> join ",""#),
        "2,4,6,8,10"
    );
}

#[test]
fn closure_args_in_pipe_sort() {
    assert_eq!(
        eval_string(r#"(5,2,8,1) |> sort { $_0 <=> $_1 } |> join ",""#),
        "1,2,5,8"
    );
}

#[test]
fn closure_args_in_pipe_reduce() {
    assert_eq!(eval_int(r#"(1..5) |> reduce { $_0 + $_1 }"#), 15);
    assert_eq!(eval_int(r#"(1..5) |> reduce { $_0 * $_1 }"#), 120);
}

#[test]
fn closure_args_in_thread_reduce() {
    assert_eq!(eval_int(r#"thread (1..5) reduce { $_0 + $_1 }"#), 15);
    assert_eq!(eval_int(r#"thread (1..5) reduce { $_0 * $_1 }"#), 120);
}

#[test]
fn closure_args_in_thread_grep_map_sum() {
    assert_eq!(
        eval_int(r#"thread (1..10) grep { $_0 % 2 == 0 } map { $_0 * $_0 } sum"#),
        220
    );
}

#[test]
fn closure_args_zip_with_two_args() {
    assert_eq!(
        eval_string(r#"zip_with { $_0 + $_1 } [1,2,3], [10,20,30] |> join ",""#),
        "11,22,33"
    );
}

#[test]
fn closure_args_zip_with_string_concat() {
    assert_eq!(
        eval_string(r#"zip_with { "$_0:$_1" } [1,2,3], ["a","b","c"] |> join ",""#),
        "1:a,2:b,3:c"
    );
}

// ── thread macro with builtins ──

#[test]
fn thread_sum_product_mean() {
    assert_eq!(eval_int(r#"thread (1..10) sum"#), 55);
    assert_eq!(eval_int(r#"thread (1..5) product"#), 120);
    assert_eq!(eval_string(r#"thread (1..10) mean"#), "5.5");
}

#[test]
fn thread_min_max_minstr_maxstr() {
    assert_eq!(eval_int(r#"thread (5,2,8,1,9) min"#), 1);
    assert_eq!(eval_int(r#"thread (5,2,8,1,9) max"#), 9);
    assert_eq!(
        eval_string(r#"thread ("apple","banana","cherry") minstr"#),
        "apple"
    );
    assert_eq!(
        eval_string(r#"thread ("apple","banana","cherry") maxstr"#),
        "cherry"
    );
}

#[test]
fn thread_uniq_shuffle_reverse() {
    assert_eq!(
        eval_string(r#"thread (1,1,2,2,3,3) uniq |> join ",""#),
        "1,2,3"
    );
    assert_eq!(eval_string(r#"thread (1,2,3) rev |> join ",""#), "3,2,1");
}

#[test]
fn thread_pairs_pairkeys_pairvalues() {
    assert_eq!(
        eval_string(r#"thread (1,2,3,4,5,6) pairkeys |> join ",""#),
        "1,3,5"
    );
    assert_eq!(
        eval_string(r#"thread (1,2,3,4,5,6) pairvalues |> join ",""#),
        "2,4,6"
    );
}

#[test]
fn thread_grep_map_sum_chain() {
    assert_eq!(
        eval_int(r#"thread (1..10) grep { $_ % 2 == 0 } map { $_ * $_ } sum"#),
        220
    );
    assert_eq!(
        eval_int(r#"thread (1..100) grep { $_ % 7 == 0 } map { $_ * 2 } sum"#),
        1470
    );
}

#[test]
fn thread_sort_with_closure_args() {
    assert_eq!(
        eval_string(r#"thread (5,2,8,1) sort { $_0 <=> $_1 } |> join ",""#),
        "1,2,5,8"
    );
    assert_eq!(
        eval_string(r#"thread (5,2,8,1) sort { $_1 <=> $_0 } |> join ",""#),
        "8,5,2,1"
    );
}

#[test]
fn thread_string_transforms() {
    assert_eq!(eval_string(r#"thread " hello " trim uc"#), "HELLO");
    assert_eq!(eval_string(r#"thread "HELLO" lc ucfirst"#), "Hello");
    assert_eq!(eval_string(r#"thread "hello" rev"#), "olleh");
}

#[test]
fn thread_case_conversions() {
    assert_eq!(
        eval_string(r#"thread "hello_world" camel_case"#),
        "helloWorld"
    );
    assert_eq!(
        eval_string(r#"thread "helloWorld" snake_case"#),
        "hello_world"
    );
    assert_eq!(
        eval_string(r#"thread "helloWorld" kebab_case"#),
        "hello-world"
    );
}

#[test]
fn thread_arrow_block_transforms() {
    assert_eq!(eval_int(r#"thread 5 >{ $_ * 2 } >{ $_ + 10 }"#), 20);
    assert_eq!(
        eval_int(r#"thread 100 >{ $_0 / 2 } >{ $_0 + 10 } >{ $_0 * 3 }"#),
        180
    );
}

#[test]
fn thread_terminates_at_pipe() {
    // |> ends the thread macro, result can be used
    assert_eq!(eval_int(r#"my $x = thread 5 >{ $_ * 2 }; $x + 100"#), 110);
}

// ── pipe forward with new builtins ──

#[test]
fn pipe_pairs_pairkeys_pairvalues() {
    assert_eq!(
        eval_string(r#"(1,2,3,4,5,6) |> pairkeys |> join ",""#),
        "1,3,5"
    );
    assert_eq!(
        eval_string(r#"(1,2,3,4,5,6) |> pairvalues |> join ",""#),
        "2,4,6"
    );
}

#[test]
fn pipe_sum_product_mean_median() {
    assert_eq!(eval_int(r#"(1..10) |> sum"#), 55);
    assert_eq!(eval_int(r#"(1..5) |> product"#), 120);
    assert_eq!(eval_string(r#"(1..10) |> mean"#), "5.5");
    assert_eq!(eval_string(r#"(1..10) |> median"#), "5.5");
}

#[test]
fn pipe_min_max_minstr_maxstr() {
    assert_eq!(eval_int(r#"(5,2,8,1,9) |> min"#), 1);
    assert_eq!(eval_int(r#"(5,2,8,1,9) |> max"#), 9);
    assert_eq!(
        eval_string(r#"("apple","banana","cherry") |> minstr"#),
        "apple"
    );
    assert_eq!(
        eval_string(r#"("apple","banana","cherry") |> maxstr"#),
        "cherry"
    );
}

#[test]
fn pipe_shuffle_uniq() {
    // shuffle changes order but preserves elements
    assert_eq!(eval_int(r#"(1..10) |> shuffle |> sum"#), 55);
    assert_eq!(eval_string(r#"(1,1,2,2,3,3) |> uniq |> join ",""#), "1,2,3");
}

#[test]
fn pipe_long_chain_numeric() {
    assert_eq!(
        eval_int(r#"(1..50) |> grep { $_ % 2 == 1 } |> map { $_ ** 2 } |> sum |> sqrt |> int"#),
        144
    );
}

#[test]
fn pipe_long_chain_string() {
    assert_eq!(
        eval_string(r#"" hello world " |> trim |> uc |> rev |> lc |> ucfirst"#),
        "Dlrow olleh"
    );
}

// ── user-defined functions in thread ──

#[test]
fn thread_user_defined_functions_long_chain() {
    assert_eq!(
        eval_string(
            r#"fn dbl { $_0 * 2 }
               fn tripl { $_0 * 3 }
               fn add5 { $_0 + 5 }
               fn square_it { $_0 ** 2 }
               fn halve { $_0 / 2 }
               thread 2 dbl tripl add5 square_it halve"#
        ),
        "144.5"
    );
}

#[test]
fn thread_user_defined_string_functions() {
    assert_eq!(
        eval_string(
            r#"fn wrap { "[$_0]" }
               fn upper { uc($_0) }
               fn trim_ { trim($_0) }
               fn rev_ { rev($_0) }
               fn bang { "$_0!" }
               thread "  hello  " trim_ upper rev_ wrap bang"#
        ),
        "[OLLEH]!"
    );
}

#[test]
fn thread_mixed_builtins_and_udfs() {
    assert_eq!(
        eval_int(
            r#"fn dbl { $_0 * 2 }
               thread 5 dbl uc length"#
        ),
        2
    );
}

#[test]
fn pipe_stddev_variance_mode() {
    // stddev of 1..10 is ~2.87
    let s = eval_string(r#"(1..10) |> stddev"#);
    assert!(s.starts_with("2.87"));
    // mode of (1,1,2,2,2,3) is 2
    assert_eq!(eval_int(r#"(1,1,2,2,2,3) |> mode"#), 2);
}

#[test]
fn sample_returns_correct_count() {
    // sample N returns N elements
    assert_eq!(eval_int(r#"my @s = sample 3, 1..10; len(@s)"#), 3);
    assert_eq!(eval_int(r#"my @s = sample 5, 1..20; len(@s)"#), 5);
}

#[test]
fn arrow_call_passes_all_args() {
    // Verify $f->(1,2,3) passes all 3 args, not just last
    assert_eq!(
        eval_int(r#"my $f = fn { $_0 + $_1 + $_2 }; $f->(10, 20, 30)"#),
        60
    );
    assert_eq!(
        eval_string(r#"my $f = fn { "$_0-$_1-$_2" }; $f->("a", "b", "c")"#),
        "a-b-c"
    );
}

// ── __SUB__ (anonymous recursion) ──

#[test]
fn dunder_sub_basic_recursion() {
    // fib(10) = 55
    assert_eq!(
        eval_int(
            r#"my $fib = fn { my $n = $_[0]; $n < 2 ? $n : __SUB__->($n-1) + __SUB__->($n-2) };
               $fib->(10)"#
        ),
        55
    );
}

#[test]
fn dunder_sub_factorial() {
    // 5! = 120
    assert_eq!(
        eval_int(
            r#"my $fact = fn { my $n = shift; $n <= 1 ? 1 : $n * __SUB__->($n - 1) };
               $fact->(5)"#
        ),
        120
    );
}

#[test]
fn dunder_sub_undef_outside_sub() {
    // __SUB__ is undef at top level
    assert_eq!(eval_int(r#"defined(__SUB__) ? 1 : 0"#), 0);
}

#[test]
fn dunder_sub_in_named_sub() {
    // __SUB__ works in named subs too
    assert_eq!(
        eval_int(
            r#"fn countdown { my $n = shift; $n <= 0 ? 0 : 1 + __SUB__->($n - 1) } countdown(5)"#
        ),
        5
    );
}

// ── defer ──
// Note: stryke closures capture by value, not reference. Tests verify
// defer execution through computation rather than mutation.

#[test]
fn defer_basic_runs() {
    // defer block executes when scope exits; closures share the enclosing
    // variable binding, so the assignment inside defer modifies the outer $x.
    assert_eq!(
        eval_int(r#"my $x = 5; { defer { $x = 100 } } $x"#),
        100 // Perl: defer modifies shared $x
    );
}

#[test]
fn defer_computes_at_exit() {
    // defer can compute with captured values
    assert_eq!(eval_int(r#"do { my $x = 5; defer { 42 }; $x * 2 }"#), 10);
}

#[test]
fn defer_in_named_sub() {
    // defer executes before sub return
    assert_eq!(
        eval_int(
            r#"fn foo { my $result = 10; defer { 99 }; $result }
               foo()"#
        ),
        10
    );
}

#[test]
fn defer_with_explicit_return() {
    // defer still runs when sub has explicit return
    assert_eq!(
        eval_int(
            r#"fn bar { defer { 99 }; return 42 }
               bar()"#
        ),
        42
    );
}

// ── short aliases: inc, dec, rev, p, t ──

#[test]
fn inc_increments_value() {
    assert_eq!(eval_int(r#"my $x = 5; inc($x)"#), 6);
}

#[test]
fn inc_default_topic() {
    assert_eq!(eval_int(r#"$_ = 10; inc()"#), 11);
}

#[test]
fn dec_decrements_value() {
    assert_eq!(eval_int(r#"my $x = 5; dec($x)"#), 4);
}

#[test]
fn dec_default_topic() {
    assert_eq!(eval_int(r#"$_ = 10; dec()"#), 9);
}

#[test]
fn rev_reverses_string() {
    assert_eq!(eval_string(r#"rev "hello""#), "olleh");
}

#[test]
fn rev_in_pipe() {
    assert_eq!(eval_string(r#""world" |> rev"#), "dlrow");
}

#[test]
fn p_alias_for_say() {
    // p is alias for say, output captured but returns 1
    assert_eq!(eval_int(r#"p("test")"#), 1);
}

#[test]
fn t_alias_for_thread_inc_chain() {
    assert_eq!(eval_int(r#"t 5 inc inc inc"#), 8);
}

#[test]
fn t_alias_for_thread_dec_chain() {
    assert_eq!(eval_int(r#"t 10 dec dec"#), 8);
}

#[test]
fn t_thread_rev_string() {
    assert_eq!(eval_string(r#"t "abc" rev"#), "cba");
}

#[test]
fn t_thread_mixed_ops() {
    assert_eq!(eval_int(r#"t 1 inc inc dec inc"#), 3);
}

#[test]
fn inc_in_pipe_chain() {
    assert_eq!(eval_int(r#"5 |> inc |> inc"#), 7);
}

#[test]
fn dec_in_pipe_chain() {
    assert_eq!(eval_int(r#"10 |> dec |> dec |> dec"#), 7);
}

// ── pipe forward chains ──

#[test]
fn pipe_map_filter_sum() {
    // (1..10) doubled = 2,4,6,8,10,12,14,16,18,20; filter >10 = 12,14,16,18,20; sum = 80
    assert_eq!(
        eval_int(r#"(1..10) |> map { $_ * 2 } |> grep { $_ > 10 } |> sum"#),
        80
    );
}

#[test]
fn pipe_with_rev_and_join() {
    assert_eq!(eval_string(r#""hello" |> rev |> uc"#), "OLLEH");
}

#[test]
fn pipe_take_drop() {
    assert_eq!(
        eval_string(r#"(1..10) |> drop 3 |> take 4 |> join ','"#),
        "4,5,6,7"
    );
}

#[test]
fn pipe_uniq_sort() {
    assert_eq!(
        eval_string(r#"(3,1,2,1,3,2) |> uniq |> sort { $a <=> $b } |> join ','"#),
        "1,2,3"
    );
}

// ── thread macro edge cases ──

#[test]
fn thread_with_map_block() {
    assert_eq!(eval_int(r#"t (1,2,3) map { $_ * 10 } sum"#), 60);
}

#[test]
fn thread_with_grep_block() {
    assert_eq!(eval_int(r#"t (1..10) grep { $_ % 2 == 0 } sum"#), 30);
}

#[test]
fn thread_empty_list() {
    assert_eq!(eval_int(r#"t () sum"#), 0);
}

// ── thread s/// and tr/// ──

#[test]
fn thread_subst_basic() {
    assert_eq!(
        eval_string(r#"t "hello world" s/world/perl/"#),
        "hello perl"
    );
}

#[test]
fn thread_subst_global() {
    assert_eq!(eval_string(r#"t "aaa" s/a/b/g"#), "bbb");
}

#[test]
fn thread_subst_in_chain() {
    assert_eq!(eval_string(r#"t 5 inc dec sqrt squared str s/5/10/"#), "10");
}

#[test]
fn thread_subst_with_flags() {
    assert_eq!(eval_string(r#"t "tomm" s/o/b/g"#), "tbmm");
}

#[test]
fn thread_tr_basic() {
    assert_eq!(eval_string(r#"t "abc" tr/a-z/A-Z/"#), "ABC");
}

#[test]
fn thread_tr_in_chain() {
    assert_eq!(eval_string(r#"t "hello" uc tr/A-Z/a-z/"#), "hello");
}

#[test]
fn thread_subst_then_func() {
    assert_eq!(eval_string(r#"t "foo" s/o/O/g uc"#), "FOO");
}

#[test]
fn thread_match_basic() {
    assert_eq!(eval_int(r#"t "hello world" m/world/"#), 1);
}

#[test]
fn thread_match_no_match() {
    assert_eq!(eval_int(r#"t "hello" m/xyz/"#), 0);
}

#[test]
fn thread_match_case_insensitive() {
    assert_eq!(eval_int(r#"t "FOO" m/foo/i"#), 1);
}

#[test]
fn thread_match_digits() {
    assert_eq!(eval_int(r#"t "abc123" m/\d+/"#), 1);
}

// ── static analysis compatible patterns ──

#[test]
fn foreach_with_my_declares_var() {
    assert_eq!(
        eval_int(r#"my $sum = 0; foreach my $i (1..5) { $sum += $i; } $sum"#),
        15
    );
}

#[test]
fn nested_foreach_scopes() {
    assert_eq!(
        eval_int(
            r#"my $sum = 0; foreach my $i (1..3) { foreach my $j (1..3) { $sum += $i * $j; } } $sum"#
        ),
        36
    );
}

#[test]
fn try_catch_error_var() {
    // die adds " at FILE line N.\n" suffix like Perl
    assert!(
        eval_string(r#"my $msg; try { die "oops"; } catch ($e) { $msg = $e; } $msg"#)
            .starts_with("oops")
    );
}

#[test]
fn coderef_with_params() {
    assert_eq!(
        eval_int(r#"my $add = fn ($a, $b) { $a + $b }; $add->(3, 4)"#),
        7
    );
}

#[test]
fn arrow_sub_syntax() {
    assert_eq!(
        eval_int(r#"my $double = fn ($x) { $x * 2 }; $double->(21)"#),
        42
    );
}

// ── builtin aliases ──

#[test]
fn hd_alias_for_head() {
    assert_eq!(eval_string(r#"(1..10) |> hd 3 |> join ','"#), "1,2,3");
}

#[test]
fn tl_alias_for_tail() {
    assert_eq!(eval_string(r#"(1..10) |> tl 3 |> join ','"#), "8,9,10");
}

#[test]
fn uq_alias_for_uniq() {
    assert_eq!(eval_string(r#"(1,1,2,2,3,3) |> uq |> join ','"#), "1,2,3");
}

#[test]
fn rv_alias_for_reverse() {
    assert_eq!(eval_string(r#"(1,2,3) |> rv |> join ','"#), "3,2,1");
}

#[test]
fn fl_alias_for_flatten() {
    assert_eq!(
        eval_string(r#"([1,2], [3,4]) |> fl |> join ','"#),
        "1,2,3,4"
    );
}

// ── range with step ──

#[test]
fn range_with_step_ascending() {
    assert_eq!(
        eval_string(r#"range(0, 10, 2) |> join ','"#),
        "0,2,4,6,8,10"
    );
}

#[test]
fn range_with_step_descending() {
    assert_eq!(
        eval_string(r#"range(10, 0, -2) |> join ','"#),
        "10,8,6,4,2,0"
    );
}

#[test]
fn range_with_step_three() {
    assert_eq!(eval_string(r#"range(1, 10, 3) |> join ','"#), "1,4,7,10");
}

#[test]
fn range_with_step_five() {
    assert_eq!(
        eval_string(r#"range(0, 100, 5) |> take 5 |> join ','"#),
        "0,5,10,15,20"
    );
}

#[test]
fn range_without_step_ascending() {
    assert_eq!(eval_string(r#"range(1, 5) |> join ','"#), "1,2,3,4,5");
}

#[test]
fn range_without_step_descending() {
    assert_eq!(eval_string(r#"range(5, 1) |> join ','"#), "5,4,3,2,1");
}

#[test]
fn range_zero_step_treated_as_one() {
    assert_eq!(eval_string(r#"range(1, 5, 0) |> join ','"#), "1,2,3,4,5");
}

#[test]
fn range_step_in_pipeline() {
    assert_eq!(eval_int(r#"range(0, 100, 10) |> sum"#), 550);
}

#[test]
fn range_step_with_map() {
    assert_eq!(
        eval_string(r#"range(1, 9, 2) |> map { $_ * 2 } |> join ','"#),
        "2,6,10,14,18"
    );
}

#[test]
fn range_sum_pipeline() {
    assert_eq!(eval_int(r#"range(1, 100) |> sum"#), 5050);
}

#[test]
fn range_product_pipeline() {
    assert_eq!(eval_int(r#"range(1, 10) |> product"#), 3628800);
}

// ── outer topic $_< ──

#[test]
fn outer_topic_basic_map() {
    // $_< is the previous topic value (shifted per set_topic call)
    assert_eq!(
        eval_string(r#"$_ = 10; my @r = map { $_ + $_< } (1, 2, 3); join ',', @r"#),
        "11,3,5"
    );
}

#[test]
fn outer_topic_in_grep() {
    // $_< holds the previous $_ before grep set the current element
    assert_eq!(
        eval_string(r#"$_ = 3; (1, 2, 3, 4, 5) |> grep { $_ > $_< } |> join ','"#),
        "2,3,4,5"
    );
}

#[test]
fn outer_topic_double_nesting() {
    // Two levels: $_< is one up, $_<< is two up
    assert_eq!(
        eval_int(
            r#"$_ = 100;
            my @outer = map {
                my @inner = map { $_ + $_< + $_<< } (1);
                $inner[0]
            } (10);
            $outer[0]"#
        ),
        111 // 1 + 10 + 100
    );
}

#[test]
fn outer_topic_undef_when_no_enclosing() {
    // At top level with no enclosing topic, $_< should be undef
    assert_eq!(eval_int(r#"defined($_<) ? 1 : 0"#), 0);
}

#[test]
fn outer_topic_in_thread_sub_stage() {
    assert_eq!(eval_int(r#"$_ = 50; t 10 >{ $_ + $_< }"#), 60);
}

// ── nested implicit param matrix: `_N<+` reaches the Nth positional N
// frames up. Stryke-only (no other language has nested implicit
// positionals). See scope.rs::set_closure_args / set_topic. ──

#[test]
fn nested_positional_outer_topic_reaches_4_frames_up() {
    // 4 nested map blocks inside a 3-arg sub. From the innermost body,
    // _N<<<< reads the Nth positional of the outermost frame (the sub).
    // Use a global sentinel rather than nested arrayref indexing because
    // each map level wraps the result in its own list.
    assert_eq!(
        eval_string(
            r#"$captured = "";
               fn deep($_0, $_1, $_2) {
                 ~> 1:1 map { ~> 1:1 map { ~> 1:1 map { ~> 1:1 map {
                   $captured = join(",", _0<<<<, _1<<<<, _2<<<<);
                 } } } }
               }
               deep("alpha", "beta", "gamma");
               $captured"#,
        ),
        "alpha,beta,gamma"
    );
}

#[test]
fn slot_0_has_four_equivalent_spellings_at_every_level() {
    // _<< ≡ $_<< ≡ _0<< ≡ $_0<< — all four resolve to the same scalar.
    assert_eq!(
        eval_string(
            r#"$out = "";
               fn shallow($_0) {
                 ~> 1:1 map { ~> 1:1 map {
                   $out = join(",", _<<, $_<<, _0<<, $_0<<)
                 } }
               }
               shallow("X");
               $out"#,
        ),
        "X,X,X,X"
    );
}

#[test]
fn slot_n_two_spellings_per_level() {
    // _N<+ ≡ $_N<+ — two spellings per (slot, level) for slot ≥ 1.
    assert_eq!(
        eval_string(
            r#"$out = "";
               fn pair($_0, $_1) {
                 ~> 1:1 map {
                   $out = join(",", _1<, $_1<)
                 }
               }
               pair("first", "second");
               $out"#,
        ),
        "second,second"
    );
}

#[test]
fn outer_chain_propagates_undef_through_intermediate_frames() {
    // Slot 1 is bound only at the outer sub. Map iterations in between
    // shift the chain (slot 1's "current" becomes undef inside map), so
    // _1<< at the innermost reaches back to the outer sub's $_1.
    assert_eq!(
        eval_int(
            r#"$out = 0;
               fn pair($_0, $_1) {
                 ~> 1:1 map { ~> 1:1 map {
                   $out = _1<<
                 } }
               }
               pair(10, 20);
               $out"#,
        ),
        20
    );
}

#[test]
fn pmap_parallel_cartesian_with_outer_chain_golf() {
    // The canonical golf one-liner: parallel cross-product where each
    // inner iter binds `_` to its own element and `_<` to the *previous*
    // topic (the most recent value before this iter's set_topic shift).
    // For a 1-element inner array, `_<` is reliably the outer iter's
    // element — clean cartesian sum without naming variables.
    //   ~> @outer pmap { ~> @inner pmap { _< + _ } } sum
    // [1,2,3] × [10]: (1+10)+(2+10)+(3+10) = 36.
    assert_eq!(
        eval_int(
            r#"my @outer = (1, 2, 3);
               my @inner = (10);
               ~> @outer pmap { ~> @inner pmap { _< + _ } } sum"#,
        ),
        36
    );
}

#[test]
fn outer_chain_shifts_on_every_set_topic_not_on_frame() {
    // Documents the load-bearing semantic: `_<` is "the value `_` held
    // before the most recent shift", not "the enclosing frame's `_`".
    // Each map iter shifts, so a multi-element inner loop sees the
    // previous inner element as `_<` after the first inner iter.
    // For [1,2] × [10,20] with `_< + _`, traced step by step:
    //   o=1 set_topic(1) → _<=prev(undef→0), _=1
    //     i=10 set_topic(10) → _<=1, _=10 → body 11
    //     i=20 set_topic(20) → _<=10, _=20 → body 30
    //   o=2 set_topic(2) → _<=20 (last shift was inner i=20), _=2
    //     i=10 set_topic(10) → _<=2, _=10 → body 12
    //     i=20 set_topic(20) → _<=10, _=20 → body 30
    // Total 11+30+12+30 = 83.
    // Same mechanic gives "previous topic" rolling access for
    // `$_ = 10; map { $_ + $_< } (1,2,3)` → "11,3,5".
    assert_eq!(
        eval_int(
            r#"my @outer = (1, 2);
               my @inner = (10, 20);
               ~> @outer map { ~> @inner map { _< + _ } } |> flatten |> sum"#,
        ),
        83
    );
}

// ── sum/sum0/product with iterators and arrays ──

#[test]
fn sum_with_array_arg() {
    assert_eq!(eval_int(r#"my @a = (1, 2, 3); sum(@a)"#), 6);
}

#[test]
fn sum0_empty_returns_zero() {
    assert_eq!(eval_int(r#"sum0()"#), 0);
}

#[test]
fn sum_with_iterator_from_range() {
    assert_eq!(eval_int(r#"sum(range(1, 10))"#), 55);
}

#[test]
fn product_with_iterator_from_range() {
    assert_eq!(eval_int(r#"product(range(1, 6))"#), 720);
}

#[test]
fn sum0_with_array_and_scalars_mixed() {
    assert_eq!(eval_int(r#"my @a = (10, 20); sum0(@a, 5)"#), 35);
}

// ── range with step edge cases ──

#[test]
fn range_negative_step() {
    assert_eq!(
        eval_string(r#"range(10, 0, -2) |> join ','"#),
        "10,8,6,4,2,0"
    );
}

#[test]
fn range_step_not_evenly_divisible() {
    assert_eq!(eval_string(r#"range(0, 10, 3) |> join ','"#), "0,3,6,9");
}

#[test]
fn range_step_large_jump() {
    assert_eq!(eval_string(r#"range(0, 100, 50) |> join ','"#), "0,50,100");
}

#[test]
fn range_step_single_element() {
    assert_eq!(eval_string(r#"range(5, 5, 1) |> join ','"#), "5");
}

#[test]
fn range_step_descending_with_filter() {
    assert_eq!(
        eval_string(r#"range(20, 0, -5) |> grep { $_ % 2 == 0 } |> join ','"#),
        "20,10,0"
    );
}

// ── thread s/// / tr/// edge cases ──

#[test]
fn thread_subst_empty_replacement() {
    assert_eq!(eval_string(r#"t "hello" s/l//g"#), "heo");
}

#[test]
fn thread_subst_regex_special_chars() {
    assert_eq!(eval_string(r#"t "a.b.c" s/\./-/g"#), "a-b-c");
}

#[test]
fn thread_tr_partial_range() {
    assert_eq!(eval_string(r#"t "abc123" tr/a-z/A-Z/"#), "ABC123");
}

#[test]
fn thread_subst_then_tr() {
    assert_eq!(
        eval_string(r#"t "hello world" s/world/perl/ tr/a-z/A-Z/"#),
        "HELLO PERL"
    );
}

#[test]
fn thread_match_in_chain() {
    // match returns 1/0, then multiply
    assert_eq!(eval_int(r#"t "abc" m/b/ inc"#), 2);
}

#[test]
fn thread_match_no_match_in_chain() {
    assert_eq!(eval_int(r#"t "abc" m/z/ inc"#), 1);
}

// ── Block params `{ |$var| body }` ──

#[test]
fn block_params_map_single() {
    assert_eq!(
        eval_string(r#"join ",", map { |$n| $n * $n }, 1..5"#),
        "1,4,9,16,25"
    );
}

#[test]
fn block_params_grep_single() {
    assert_eq!(
        eval_string(r#"join ",", grep { |$x| $x > 3 }, 1..6"#),
        "4,5,6"
    );
}

#[test]
fn block_params_sort_two() {
    assert_eq!(
        eval_string(r#"join ",", sort { |$x, $y| $y <=> $x }, 3, 1, 4, 1, 5"#),
        "5,4,3,1,1"
    );
}

#[test]
fn block_params_reduce_two() {
    assert_eq!(
        eval_int(r#"reduce { |$acc, $val| $acc + $val }, 1..10"#),
        55
    );
}

#[test]
fn block_params_map_pipe() {
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> map { |$n| $n + 10 } |> join ",""#),
        "11,12,13"
    );
}
