use crate::common::*;

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

#[test]
fn parallel_grep() {
    let result = eval("my @a = pgrep { $_ % 2 == 0 } (1,2,3,4,5,6); scalar @a");
    assert_eq!(result.to_int(), 3);
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
fn fan_zero_iterations_skips_block() {
    assert_eq!(eval_int(r#"fan 0 { die "should not run" }; 1"#), 1);
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
