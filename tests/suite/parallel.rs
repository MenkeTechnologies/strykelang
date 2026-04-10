use crate::common::*;
use perlrs::error::ErrorKind;

#[test]
fn parallel_map() {
    let result = eval("my @a = pmap { $_ * 2 } (1,2,3,4,5); scalar @a");
    assert_eq!(result.to_int(), 5);
}

#[test]
fn parallel_map_preserves_input_order_in_results() {
    assert_eq!(
        eval_string(r#"join(",", pmap { $_ * 2 } (1,2,3,4))"#),
        "2,4,6,8"
    );
}

#[test]
fn parallel_map_progress_flag_runs() {
    assert_eq!(
        eval_string(r#"join(",", pmap { $_ * 2 } (1,2,3,4), progress => 0)"#),
        "2,4,6,8"
    );
}

/// Failed `pmap` elements stringify as empty between commas; order matches the input list.
#[test]
fn parallel_map_mixed_undef_slots_preserve_positions() {
    assert_eq!(
        eval_string(r#"join(",", pmap { $_ == 1 ? 1/0 : $_ * 2 } (1, 2, 3))"#),
        ",4,6",
    );
}

#[test]
fn pmap_chunked_preserves_input_order() {
    assert_eq!(
        eval_string(r#"join(",", pmap_chunked 2 { $_ * 2 } (1, 2, 3, 4))"#),
        "2,4,6,8",
    );
}

/// Chunk size larger than the list still processes every element in order.
#[test]
fn pmap_chunked_large_chunk_small_list() {
    assert_eq!(
        eval_string(r#"join(",", pmap_chunked 99 { $_ * 2 } (1, 2))"#),
        "2,4",
    );
}

/// Matched elements stay in original list order (not sorted by value).
#[test]
fn parallel_grep_preserves_input_order() {
    assert_eq!(
        eval_string(r#"join(",", pgrep { $_ > 1 } (3, 1, 4))"#),
        "3,4",
    );
}

#[test]
fn parallel_grep() {
    let result = eval("my @a = pgrep { $_ % 2 == 0 } (1,2,3,4,5,6); scalar @a");
    assert_eq!(result.to_int(), 3);
}

#[test]
fn parallel_grep_progress_flag_runs() {
    assert_eq!(
        eval_string(r#"join(",", pgrep { $_ % 2 == 0 } (1, 2, 3, 4), progress => 0)"#),
        "2,4",
    );
}

#[test]
fn parallel_map_empty_list() {
    assert_eq!(eval_int(r#"scalar pmap { $_ } ()"#), 0);
}

#[test]
fn parallel_grep_empty_list() {
    assert_eq!(eval_int(r#"scalar pgrep { 1 } ()"#), 0);
}

#[test]
fn parallel_sort_empty_list() {
    assert_eq!(eval_int(r#"scalar psort ()"#), 0);
}

#[test]
fn pmap_chunked_empty_list() {
    assert_eq!(eval_int(r#"scalar pmap_chunked 4 { $_ } ()"#), 0);
}

/// `pmap` keeps result length; block failures become `undef` per element (VM `Err` → `UNDEF`).
#[test]
fn parallel_map_keeps_length_when_block_errors() {
    assert_eq!(eval_int(r#"scalar pmap { 1/0 } (1, 2, 3)"#), 3);
}

/// `pgrep` treats block failure as false — the input item is not kept.
#[test]
fn parallel_grep_drops_items_when_block_errors() {
    assert_eq!(eval_int(r#"scalar pgrep { 1/0 } (1, 2, 3)"#), 0);
}

/// Only the item whose predicate errors is excluded; others still match.
#[test]
fn parallel_grep_mixed_errors_and_successes() {
    assert_eq!(
        eval_int(r#"my @a = pgrep { $_ == 2 ? 1/0 : $_ > 0 } (1, 2, 3); scalar @a"#),
        2,
    );
}

#[test]
fn parallel_map_single_element() {
    assert_eq!(eval_string(r#"join(",", pmap { $_ * 2 } (21))"#), "42");
}

/// Captured non-`mysync` lexicals cannot be assigned from parallel workers (`Scope::parallel_guard`).
/// Use `pfor` here: `pmap` / `pgrep` swallow per-element block failures (`undef` / drop), so guard
/// violations would not surface as the program result.
#[test]
fn parallel_block_rejects_captured_lexical_assignment() {
    assert_eq!(
        eval_err_kind(r#"my $x = 0; pfor { $x = 1 } (1); 1"#),
        ErrorKind::Runtime,
    );
}

#[test]
fn parallel_block_allows_mysync_scalar_mutation() {
    assert_eq!(eval_int(r#"mysync $c = 0; pmap { $c++ } (1, 2, 3); $c"#), 3);
}

/// `mysync` compound assignment in `pmap` — full RMW under the atomic lock, no parallel-guard error.
#[test]
fn parallel_mysync_compound_assign_in_pmap() {
    assert_eq!(
        eval_int(r#"mysync $c = 0; pmap { $c += $_ } (1, 2, 3); $c"#),
        6
    );
}

/// `mysync` scalar may be updated in `pgrep` workers (predicate runs once per element).
#[test]
fn parallel_mysync_mutation_in_pgrep() {
    assert_eq!(
        eval_int(r#"mysync $n = 0; pgrep { $n++; $_ > 1 } (1, 2, 3); $n"#),
        3
    );
}

#[test]
fn parallel_mysync_increment_in_pmap_chunked() {
    assert_eq!(
        eval_int(r#"mysync $c = 0; pmap_chunked 2 { $c++ } (1, 2, 3); $c"#),
        3
    );
}

/// Shared `mysync` array — `push` from `pmap` workers is serialized via the atomic array path.
#[test]
fn parallel_mysync_array_push_in_pmap() {
    assert_eq!(
        eval_int(r#"mysync @a; pmap { push @a, $_ } (1, 2, 3); scalar @a"#),
        3
    );
}

/// Shared `mysync` hash — element updates from `pfor` workers.
#[test]
fn parallel_mysync_hash_buckets_in_pfor() {
    assert_eq!(
        eval_int(
            r#"mysync %bucket; pfor { $bucket{$_ % 2} += 1 } (0..9); $bucket{0} + $bucket{1}"#
        ),
        10
    );
}

/// `pfor` with `mysync` mutation succeeds (same guard rules as `pmap`, but `pfor` propagates errors).
#[test]
fn parallel_pfor_mysync_increment_no_error() {
    assert_eq!(eval_int(r#"mysync $c = 0; pfor { $c++ } (1, 2, 3); $c"#), 3);
}

#[test]
fn parallel_grep_single_element() {
    assert_eq!(eval_int(r#"scalar pgrep { $_ > 0 } (7)"#), 1);
}

#[test]
fn parallel_sort_single_element_unchanged() {
    assert_eq!(eval_string(r#"join(",", psort { $a <=> $b } (99))"#), "99");
}

#[test]
fn parallel_sort() {
    assert_eq!(
        eval_string(r#"join(",", psort { $a <=> $b } (5,3,1,4,2))"#),
        "1,2,3,4,5"
    );
}

#[test]
fn parallel_sort_default_string_order() {
    assert_eq!(eval_string(r#"join(",", psort ("c","a","b"))"#), "a,b,c");
}

#[test]
fn parallel_for_runs() {
    assert_eq!(eval_int("pfor { $_ } (1,2,3); 99"), 99);
}

#[test]
fn par_lines_invokes_block_per_line_with_mysync_count() {
    let dir = std::env::temp_dir().join(format!("perlrs_par_lines_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("lines.txt");
    std::fs::write(&p, "a,b\n2,3").unwrap();
    let path = p.to_str().unwrap();
    let code = format!(r#"mysync $n = 0; par_lines "{path}", sub {{ $n++ }}; $n"#);
    assert_eq!(eval_int(&code), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn par_walk_visits_files_and_dirs_with_mysync_count() {
    let dir = std::env::temp_dir().join(format!("perlrs_par_walk_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::write(dir.join("a/x.txt"), "1").unwrap();
    std::fs::write(dir.join("root.txt"), "2").unwrap();
    let path = dir.to_str().unwrap();
    let code = format!(r#"mysync $n = 0; par_walk "{path}", sub {{ $n++ }}; $n"#);
    // root dir, root.txt, subdir a, a/x.txt = 4 paths
    assert_eq!(eval_int(&code), 4);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn par_sed_rewrites_multiple_files_in_parallel() {
    let dir = std::env::temp_dir().join(format!("perlrs_par_sed_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let f1 = dir.join("one.txt");
    let f2 = dir.join("two.txt");
    std::fs::write(&f1, "foo bar").unwrap();
    std::fs::write(&f2, "foo baz").unwrap();
    let p1 = f1.to_str().unwrap();
    let p2 = f2.to_str().unwrap();
    let code = format!(
        r#"my $k = par_sed("foo", "ZZ", "{p1}", "{p2}"); $k . ":" . slurp("{p1}") . slurp("{p2}")"#
    );
    let out = eval_string(&code);
    assert_eq!(out, "2:ZZ barZZ baz");
    std::fs::remove_dir_all(&dir).ok();
}

/// `fan { }` iterates `$_` from `0` to `rayon::current_num_threads() - 1` (same pool as `pe -j`).
#[test]
fn fan_default_count_matches_rayon_thread_pool() {
    let n = rayon::current_num_threads();
    let expected = (n * n.saturating_sub(1) / 2) as i64;
    assert_eq!(
        eval_int(r#"sub pr { $s += $_ } mysync $s = 0; fan { pr }; $s"#),
        expected,
    );
}

#[test]
fn fan_zero_iterations_skips_block() {
    assert_eq!(eval_int(r#"fan 0 { die "should not run" }; 1"#), 1);
}

#[test]
fn fan_cap_collects_return_values_in_index_order() {
    assert_eq!(eval_string(r#"join ",", fan_cap 4 { $_ * 2 }"#), "0,2,4,6");
}

#[test]
fn fan_cap_zero_iterations_yields_empty_list() {
    assert_eq!(eval_int(r#"my @a = fan_cap 0 { die "no" }; scalar @a"#), 0);
}

#[test]
fn pfor_empty_list_skips_block() {
    assert_eq!(eval_int(r#"pfor { die "should not run" } (); 1"#), 1);
}

#[test]
fn pfor_undefined_bareword_sub_is_runtime_error() {
    assert_eq!(
        eval_err_kind(r#"pfor { nosuchsub } (1); 1"#),
        ErrorKind::Runtime,
    );
}

#[test]
fn fan_undefined_bareword_sub_is_runtime_error() {
    assert_eq!(
        eval_err_kind(r#"fan 1 { nosuchsub }; 1"#),
        ErrorKind::Runtime,
    );
}

#[test]
fn pfor_die_in_worker_is_die_kind() {
    assert_eq!(
        eval_err_kind(r#"pfor { die "worker" } (1); 1"#),
        ErrorKind::Die,
    );
}

#[test]
fn fan_die_in_worker_is_die_kind() {
    assert_eq!(
        eval_err_kind(r#"fan 1 { die "worker" }; 1"#),
        ErrorKind::Die,
    );
}

/// Bareword `{ processme }` is a zero-arg sub call; `@_` is `($_)` (fan worker index 0..N-1).
#[test]
fn fan_bareword_sub_passes_worker_index_as_topic() {
    assert_eq!(
        eval_int(r#"sub processme { $s += $_ } mysync $s = 0; fan 50 { processme }; $s"#),
        1225,
    );
}

/// Bareword `{ processme }` in `pfor` — zero-arg call; `@_` is `($_)` for each list element.
#[test]
fn pfor_bareword_sub_passes_list_item_as_topic() {
    assert_eq!(
        eval_int(r#"sub processme { $s += $_ } mysync $s = 0; pfor { processme } (0..49); $s"#),
        1225,
    );
}

#[test]
fn parallel_reduce_sum() {
    assert_eq!(eval_int("preduce { $a + $b } (1,2,3,4,5)"), 15);
}

#[test]
fn parallel_reduce_product() {
    assert_eq!(eval_int("preduce { $a * $b } (1,2,3,4,5)"), 120);
}

#[test]
fn parallel_reduce_max() {
    assert_eq!(eval_int("preduce { $a > $b ? $a : $b } (3,7,1,9,2)"), 9);
}

#[test]
fn parallel_reduce_single_element() {
    assert_eq!(eval_int("preduce { $a + $b } (42)"), 42);
}

#[test]
fn parallel_reduce_empty_list_returns_undef() {
    assert_eq!(eval_int("defined(preduce { $a + $b } ()) ? 1 : 0"), 0);
}

#[test]
fn parallel_reduce_string_concat() {
    assert_eq!(eval_string(r#"preduce { $a . $b } ("a","b","c")"#), "abc");
}

#[test]
fn parallel_reduce_with_array_variable() {
    assert_eq!(
        eval_int("my @nums = (10, 20, 30); preduce { $a + $b } @nums"),
        60
    );
}

#[test]
fn preduce_init_empty_returns_identity() {
    assert_eq!(eval_int("preduce_init 0, { $a + $b } ()"), 0);
}

#[test]
fn preduce_init_single_element_folds_from_identity() {
    assert_eq!(eval_int("preduce_init 0, { $a + $b } (9)"), 9);
}

#[test]
fn preduce_init_histogram_merges_partials() {
    assert_eq!(
        eval_int(r#"my $h = preduce_init {}, { $a->{$b}++; $a } ("a","b","a"); $h->{a}"#),
        2
    );
    assert_eq!(
        eval_int(r#"my $h = preduce_init {}, { $a->{$b}++; $a } ("a","b","a"); $h->{b}"#),
        1
    );
    assert_eq!(
        eval_int(
            r#"my $h = preduce_init {}, { my ($acc, $item) = @_; $acc->{$item}++; $acc } ("x","y","x"); $h->{"x"}"#
        ),
        2
    );
}

#[test]
fn barrier_builtin_returns_barrier_value() {
    assert_eq!(eval("barrier(2)").type_name(), "Barrier");
}

#[test]
fn barrier_wait_returns_truthy_scalar() {
    assert_eq!(eval_int(r#"my $b = barrier(1); $b->wait"#), 1);
}

#[test]
fn preduce_two_elements_folds_pair() {
    assert_eq!(eval_int(r#"preduce { $a + $b } (3, 5)"#), 8);
}

#[test]
fn async_await_returns_block_value() {
    assert_eq!(eval_int(r#"my $t = async { 40 + 2 }; await($t)"#), 42,);
}

#[test]
fn timer_reports_elapsed_time() {
    assert_eq!(eval_int(r#"0+((timer { 1 }) > 0)"#), 1);
}

#[test]
fn trace_returns_block_value() {
    assert_eq!(eval_int(r#"trace { 7 + 1 }"#), 8);
}
