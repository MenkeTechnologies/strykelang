use crate::common::*;
use stryke::error::ErrorKind;

#[test]
fn parallel_map() {
    let result = eval("my @a = pmap { $_ * 2 } (1,2,3,4,5); len(@a)");
    assert_eq!(result.to_int(), 5);
}

#[test]
fn parallel_map_preserves_input_order_in_results() {
    assert_eq!(
        eval_string(r#"(1,2,3,4) |> pmap { $_ * 2 } |> join ','"#),
        "2,4,6,8"
    );
}

#[test]
fn parallel_map_progress_flag_runs() {
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> pmap { $_ * 2 }, progress => 0 |> join ','"#),
        "2,4,6,8"
    );
}

/// Failed `pmap` elements stringify as empty between commas; order matches the input list.
#[test]
fn parallel_map_mixed_undef_slots_preserve_positions() {
    assert_eq!(
        eval_string(r#"(1, 2, 3) |> pmap { $_ == 1 ? 1/0 : $_ * 2 } |> join ','"#),
        ",4,6",
    );
}

#[test]
fn pmap_chunked_preserves_input_order() {
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> pmap_chunked 2 { $_ * 2 } |> join ','"#),
        "2,4,6,8",
    );
}

/// Chunk size larger than the list still processes every element in order.
#[test]
fn pmap_chunked_large_chunk_small_list() {
    assert_eq!(
        eval_string(r#"(1, 2) |> pmap_chunked 99 { $_ * 2 } |> join ','"#),
        "2,4",
    );
}

/// Matched elements stay in original list order (not sorted by value).
#[test]
fn parallel_grep_preserves_input_order() {
    assert_eq!(
        eval_string(r#"(3, 1, 4) |> pgrep { $_ > 1 } |> join ','"#),
        "3,4",
    );
}

#[test]
fn parallel_grep() {
    let result = eval("my @a = pgrep { $_ % 2 == 0 } (1,2,3,4,5,6); len(@a)");
    assert_eq!(result.to_int(), 3);
}

#[test]
fn parallel_grep_progress_flag_runs() {
    assert_eq!(
        eval_string(r#"(1, 2, 3, 4) |> pgrep { $_ % 2 == 0 }, progress => 0 |> join ','"#),
        "2,4",
    );
}

#[test]
fn parallel_map_empty_list() {
    assert_eq!(eval_int(r#"len pmap { $_ } ()"#), 0);
}

#[test]
fn parallel_grep_empty_list() {
    assert_eq!(eval_int(r#"len pgrep { 1 } ()"#), 0);
}

#[test]
fn parallel_sort_empty_list() {
    assert_eq!(eval_int(r#"len psort ()"#), 0);
}

/// Regression: blockless `psort` as a thread-macro stage must build a `PSortExpr`,
/// not a generic `FuncCall { name: "psort" }` (which fails at runtime with
/// "Undefined subroutine &psort"). See `thread_apply_bare_func` in parser.rs.
#[test]
fn parallel_sort_thread_macro_blockless_stage() {
    assert_eq!(
        crate::common::eval_string(r#"my @a = (3, 1, 2); join(",", ~> @a psort)"#),
        "1,2,3"
    );
}

/// Same regression — string sort path through the thread macro.
#[test]
fn parallel_sort_thread_macro_strings() {
    assert_eq!(
        crate::common::eval_string(
            r#"my @a = ("banana", "apple", "cherry"); join(",", ~> @a psort)"#
        ),
        "apple,banana,cherry"
    );
}

/// Block-form builtins (`pfirst`/`pany`/`any`/`all`/`none`/`first`/`take_while`/
/// `drop_while`/`reject`/`tap`/`peek`/`group_by`/`chunk_by`/`partition`/`min_by`/
/// `max_by`/`zip_with`/`count_by`) used to fail in thread context with
/// `\`NAME\` needs { BLOCK }, LIST so the list can receive the pipe`. The
/// `parse_thread_stage_with_block` default arm only emitted a one-arg `FuncCall`,
/// then `pipe_forward_apply` rejected it (args.len() < 2). Now both sites share
/// `is_block_then_list_pipe_builtin` and the placeholder list slot is reserved.
#[test]
fn block_form_pipe_builtins_work_as_thread_stage() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) any { $_ > 2 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) all { $_ > 0 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) none { $_ > 10 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) first { $_ > 2 }"#), 3);
    assert_eq!(eval_int(r#"~> (1,2,3,4) pfirst { $_ > 2 }"#), 3);
    assert_eq!(eval_int(r#"~> (1,2,3,4) pany { $_ > 3 }"#), 1);
}

#[test]
fn block_form_pipe_builtins_work_via_pipe_forward() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"(1,2,3,4) |> any { $_ > 2 }"#), 1);
    assert_eq!(eval_int(r#"(1,2,3,4) |> first { $_ > 2 }"#), 3);
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-builtin regression matrix for the 18 block-form pipe builtins.
// Each builtin gets one `~>` assertion and one `|>` assertion to lock down both
// dispatch paths (parse_thread_stage_with_block default arm + pipe_forward_apply).
// Output values are the empirically observed outputs from the audit's probe.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn thread_stage_any_truthy_match() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) any { $_ > 3 }"#), 1);
    assert_eq!(eval_int(r#"(1,2,3,4) |> any { $_ > 3 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) any { $_ > 99 }"#), 0);
}

#[test]
fn thread_stage_all_universal_match() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) all { $_ > 0 }"#), 1);
    assert_eq!(eval_int(r#"(1,2,3,4) |> all { $_ > 0 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) all { $_ > 1 }"#), 0);
}

#[test]
fn thread_stage_none_universal_no_match() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) none { $_ > 99 }"#), 1);
    assert_eq!(eval_int(r#"(1,2,3,4) |> none { $_ > 99 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) none { $_ > 0 }"#), 0);
}

#[test]
fn thread_stage_first_existential_value() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) first { $_ > 2 }"#), 3);
    assert_eq!(eval_int(r#"(1,2,3,4) |> first { $_ > 2 }"#), 3);
}

#[test]
fn thread_stage_pfirst_parallel_existential() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) pfirst { $_ > 2 }"#), 3);
    assert_eq!(eval_int(r#"(1,2,3,4) |> pfirst { $_ > 2 }"#), 3);
}

#[test]
fn thread_stage_pany_parallel_existential() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1,2,3,4) pany { $_ > 3 }"#), 1);
    assert_eq!(eval_int(r#"(1,2,3,4) |> pany { $_ > 3 }"#), 1);
    assert_eq!(eval_int(r#"~> (1,2,3,4) pany { $_ > 99 }"#), 0);
}

#[test]
fn thread_stage_take_while_stops_at_first_falsy() {
    use crate::common::eval_string;
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,3,0,4) take_while { $_ > 0 }; join(",", @r)"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"my @r = (1,2,3,0,4) |> take_while { $_ > 0 }; join(",", @r)"#),
        "1,2,3"
    );
}

#[test]
fn thread_stage_drop_while_skips_leading_truthy() {
    use crate::common::eval_string;
    assert_eq!(
        eval_string(r#"my @r = ~> (0,0,1,2,3) drop_while { $_ == 0 }; join(",", @r)"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"my @r = (0,0,1,2,3) |> drop_while { $_ == 0 }; join(",", @r)"#),
        "1,2,3"
    );
}

#[test]
fn thread_stage_skip_while_alias_of_drop_while() {
    use crate::common::eval_string;
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,5,3,4) skip_while { $_ < 3 }; join(",", @r)"#),
        "5,3,4"
    );
    assert_eq!(
        eval_string(r#"my @r = (1,2,5,3,4) |> skip_while { $_ < 3 }; join(",", @r)"#),
        "5,3,4"
    );
}

#[test]
fn thread_stage_reject_keeps_falsy_elements() {
    use crate::common::eval_string;
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,3,4) reject { $_ > 2 }; join(",", @r)"#),
        "1,2"
    );
    assert_eq!(
        eval_string(r#"my @r = (1,2,3,4) |> reject { $_ > 2 }; join(",", @r)"#),
        "1,2"
    );
}

#[test]
fn thread_stage_tap_returns_original_list_unchanged() {
    use crate::common::eval_string;
    // `tap` runs the block for side effects; the threaded list passes through.
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,3) tap { $_ * 100 }; join(",", @r)"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"my @r = (1,2,3) |> tap { $_ * 100 }; join(",", @r)"#),
        "1,2,3"
    );
}

#[test]
fn thread_stage_peek_returns_original_list_unchanged() {
    use crate::common::eval_string;
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,3) peek { $_ * 100 }; join(",", @r)"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"my @r = (1,2,3) |> peek { $_ * 100 }; join(",", @r)"#),
        "1,2,3"
    );
}

#[test]
fn thread_stage_partition_pass_fail_arrayrefs() {
    use crate::common::eval_string;
    assert_eq!(
        eval_string(
            r#"my ($pass, $fail) = ~> (1,2,3,4) partition { $_ > 2 };
               join(",", @$pass) . "|" . join(",", @$fail)"#
        ),
        "3,4|1,2"
    );
    assert_eq!(
        eval_string(
            r#"my ($pass, $fail) = (1,2,3,4) |> partition { $_ > 2 };
               join(",", @$pass) . "|" . join(",", @$fail)"#
        ),
        "3,4|1,2"
    );
}

#[test]
fn thread_stage_min_by_smallest_key() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1, -5, 3, -2) min_by { abs($_) }"#), 1);
    assert_eq!(eval_int(r#"(1, -5, 3, -2) |> min_by { abs($_) }"#), 1);
}

#[test]
fn thread_stage_max_by_largest_key() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> (1, -5, 3, -2) max_by { abs($_) }"#), -5);
    assert_eq!(eval_int(r#"(1, -5, 3, -2) |> max_by { abs($_) }"#), -5);
}

#[test]
fn thread_stage_count_by_returns_keyed_hash() {
    use crate::common::eval_int;
    // `count_by` returns a hashref counting elements per key.
    assert_eq!(
        eval_int(r#"my $h = ~> (1,2,3,4,5) count_by { $_ % 2 }; $h->{0}"#),
        2
    );
    assert_eq!(
        eval_int(r#"my $h = ~> (1,2,3,4,5) count_by { $_ % 2 }; $h->{1}"#),
        3
    );
    assert_eq!(
        eval_int(r#"my $h = (1,2,3,4,5) |> count_by { $_ % 2 }; $h->{1}"#),
        3
    );
}

#[test]
fn thread_stage_chunk_by_runs_of_consecutive_equal_keys() {
    use crate::common::eval_int;
    // `(1,1,2,2,3,1) chunk_by { $_ }` → 4 runs: [1,1], [2,2], [3], [1].
    assert_eq!(
        eval_int(r#"my @r = ~> (1,1,2,2,3,1) chunk_by { $_ }; len(@r)"#),
        4
    );
    assert_eq!(
        eval_int(r#"my @r = (1,1,2,2,3,1) |> chunk_by { $_ }; len(@r)"#),
        4
    );
}

#[test]
fn thread_stage_group_by_does_not_error() {
    use crate::common::eval_string;
    // Lock the routing, not the exact return shape (which is intentionally
    // group-key-keyed and may evolve). Just confirm it doesn't raise.
    let r = eval_string(r#"my @r = ~> (1,2,3,4,5) group_by { $_ % 2 }; len(@r)"#);
    assert!(!r.is_empty());
    let r = eval_string(r#"my @r = (1,2,3,4,5) |> group_by { $_ % 2 }; len(@r)"#);
    assert!(!r.is_empty());
}

#[test]
fn thread_stage_zip_with_pairs_lists() {
    use crate::common::eval_int;
    // `zip_with { ... }` over a list of arrayrefs — verify routing doesn't error
    // and produces a non-empty result.
    let n = crate::common::eval_int(r#"my @r = ~> ([1,2,3]) zip_with { [$_] }; len(@r)"#);
    assert!(n > 0);
    let n = eval_int(r#"my @r = ([1,2,3]) |> zip_with { [$_] }; len(@r)"#);
    assert!(n > 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Empty-list edge cases for the predicate builtins (any/all/none/first).
// `all` and `none` are vacuously true on empty lists; `any` and `first` are not.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn thread_stage_any_empty_list_is_false() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> () any { 1 }"#), 0);
    assert_eq!(eval_int(r#"() |> any { 1 }"#), 0);
}

#[test]
fn thread_stage_all_empty_list_is_vacuously_true() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> () all { 1 }"#), 1);
    assert_eq!(eval_int(r#"() |> all { 1 }"#), 1);
}

#[test]
fn thread_stage_none_empty_list_is_vacuously_true() {
    use crate::common::eval_int;
    assert_eq!(eval_int(r#"~> () none { 1 }"#), 1);
    assert_eq!(eval_int(r#"() |> none { 1 }"#), 1);
}

#[test]
fn thread_stage_first_empty_list_is_undef() {
    use crate::common::eval_int;
    assert_eq!(
        eval_int(r#"my $r = ~> () first { 1 }; defined($r) ? 1 : 0"#),
        0
    );
    assert_eq!(
        eval_int(r#"my $r = () |> first { 1 }; defined($r) ? 1 : 0"#),
        0
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Composition: chained block-form stages must keep working when piped together.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn thread_stage_chained_block_form_composes() {
    use crate::common::eval_string;
    // `(1..6) → reject odd → take_while < 6 → first` should pick the first
    // even number under 6, i.e. 2.
    assert_eq!(
        eval_string(r#"~> (1,2,3,4,5,6) reject { $_ % 2 } take_while { $_ < 6 } first { $_ > 0 }"#),
        "2"
    );
}

#[test]
fn thread_stage_paren_form_with_arg_for_head_take_join() {
    // The audit confirmed `head(N)`, `take(N)`, `join(SEP)` require paren form
    // in `~>` (bareword positional args go to the next stage, by design).
    use crate::common::eval_string;
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,3,4,5) head(3); join(",", @r)"#),
        "1,2,3"
    );
    assert_eq!(
        eval_string(r#"my @r = ~> (1,2,3,4,5) take(2); join(",", @r)"#),
        "1,2"
    );
    assert_eq!(eval_string(r#"~> (1,2,3) join(",")"#), "1,2,3");
}

// ─────────────────────────────────────────────────────────────────────────────
// pmap_on / pflat_map_on parser dispatch. Full execution requires a wired
// `cluster(...)` runtime constructor that doesn't exist yet (LSP docs reference
// it but no runtime sub is registered). Parse-only coverage in
// parse_accepts_parallel locks the AST shape; here we confirm the parser path
// yields a *runtime* error (not a parse error or "Undefined subroutine
// &pmap_on") when handed an undef cluster — that's the pre-fix failure mode.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn thread_stage_pmap_on_undef_cluster_runtime_error() {
    use crate::common::eval_err_kind;
    use stryke::error::ErrorKind;
    assert_eq!(
        eval_err_kind(r#"my $c = undef; my @r = ~> (1,2,3) pmap_on $c { $_ * 2 }; 0"#),
        ErrorKind::Runtime
    );
}

#[test]
fn thread_stage_pmap_on_with_comma_undef_cluster_runtime_error() {
    use crate::common::eval_err_kind;
    use stryke::error::ErrorKind;
    // The optional comma between cluster and block (canonical LSP-doc form)
    // must parse identically to the no-comma form.
    assert_eq!(
        eval_err_kind(r#"my $c = undef; my @r = ~> (1,2,3) pmap_on $c, { $_ * 2 }; 0"#),
        ErrorKind::Runtime
    );
}

#[test]
fn thread_stage_pflat_map_on_undef_cluster_runtime_error() {
    use crate::common::eval_err_kind;
    use stryke::error::ErrorKind;
    assert_eq!(
        eval_err_kind(r#"my $c = undef; my @r = ~> (1,2,3) pflat_map_on $c { ($_, $_) }; 0"#),
        ErrorKind::Runtime
    );
}

#[test]
fn pipe_forward_pmap_on_undef_cluster_runtime_error() {
    use crate::common::eval_err_kind;
    use stryke::error::ErrorKind;
    assert_eq!(
        eval_err_kind(r#"my $c = undef; my @r = (1,2,3) |> pmap_on $c { $_ * 2 }; 0"#),
        ErrorKind::Runtime
    );
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
        eval_int(r#"my @a = pgrep { $_ == 2 ? 1/0 : $_ > 0 } (1, 2, 3); len(@a)"#),
        2,
    );
}

#[test]
fn parallel_map_single_element() {
    assert_eq!(eval_string(r#"(21) |> pmap { $_ * 2 } |> join ','"#), "42");
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
        eval_int(r#"mysync @a; pmap { push @a, $_ } (1, 2, 3); len(@a)"#),
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
    assert_eq!(
        eval_string(r#"(99) |> psort { $a <=> $b } |> join ','"#),
        "99"
    );
}

#[test]
fn parallel_sort() {
    assert_eq!(
        eval_string(r#"(5,3,1,4,2) |> psort { $a <=> $b } |> join ','"#),
        "1,2,3,4,5"
    );
}

#[test]
fn parallel_sort_default_string_order() {
    assert_eq!(
        eval_string(r#"("c","a","b") |> psort |> join ','"#),
        "a,b,c"
    );
}

#[test]
fn parallel_for_runs() {
    assert_eq!(eval_int("pfor { $_ } (1,2,3); 99"), 99);
}

#[test]
fn par_lines_invokes_block_per_line_with_mysync_count() {
    let dir = std::env::temp_dir().join(format!("stryke_par_lines_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("lines.txt");
    std::fs::write(&p, "a,b\n2,3").unwrap();
    let path = p.to_str().unwrap();
    let code = format!(r#"mysync $n = 0; par_lines "{path}", fn {{ $n++ }}; $n"#);
    assert_eq!(eval_int(&code), 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// Bare block + `if /re/` must not trip the parallel guard on regex capture scalars (`$&`, …).
#[test]
fn par_lines_bare_block_say_if_regex() {
    let dir = std::env::temp_dir().join(format!("stryke_par_lines_re_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("big.log");
    std::fs::write(&p, "ok\nERR x\n").unwrap();
    let path = p.to_str().unwrap();
    let code = format!(r#"mysync $o = ""; par_lines "{path}", {{ $o .= $_ if /ERR/ }}; $o"#);
    assert_eq!(eval_string(&code), "ERR x");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn par_walk_visits_files_and_dirs_with_mysync_count() {
    let dir = std::env::temp_dir().join(format!("stryke_par_walk_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::write(dir.join("a/x.txt"), "1").unwrap();
    std::fs::write(dir.join("root.txt"), "2").unwrap();
    let path = dir.to_str().unwrap();
    let code = format!(r#"mysync $n = 0; par_walk "{path}", fn {{ $n++ }}; $n"#);
    // root dir, root.txt, subdir a, a/x.txt = 4 paths
    assert_eq!(eval_int(&code), 4);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn par_sed_rewrites_multiple_files_in_parallel() {
    let dir = std::env::temp_dir().join(format!("stryke_par_sed_{}", std::process::id()));
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

/// `fan { }` iterates `$_` from `0` to `rayon::current_num_threads() - 1` (same pool as `stryke -j`).
#[test]
fn fan_default_count_matches_rayon_thread_pool() {
    let n = rayon::current_num_threads();
    let expected = (n * n.saturating_sub(1) / 2) as i64;
    assert_eq!(
        eval_int(r#"fn prto { $s += $_ } mysync $s = 0; fan { prto }; $s"#),
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
    assert_eq!(eval_int(r#"my @a = fan_cap 0 { die "no" }; len(@a)"#), 0);
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
        eval_int(r#"fn processme { $s += $_ } mysync $s = 0; fan 50 { processme }; $s"#),
        1225,
    );
}

/// Bareword `{ processme }` in `pfor` — zero-arg call; `@_` is `($_)` for each list element.
#[test]
fn pfor_bareword_sub_passes_list_item_as_topic() {
    assert_eq!(
        eval_int(r#"fn processme { $s += $_ } mysync $s = 0; pfor { processme } (0..49); $s"#),
        1225,
    );
}

#[test]
fn parallel_reduce_sum() {
    assert_eq!(eval_int("(1,2,3,4,5) |> preduce { $a + $b }"), 15);
}

#[test]
fn parallel_reduce_product() {
    assert_eq!(eval_int("(1,2,3,4,5) |> preduce { $a * $b }"), 120);
}

#[test]
fn parallel_reduce_max() {
    assert_eq!(eval_int("(3,7,1,9,2) |> preduce { $a > $b ? $a : $b }"), 9);
}

#[test]
fn parallel_reduce_single_element() {
    assert_eq!(eval_int("(42) |> preduce { $a + $b }"), 42);
}

#[test]
fn parallel_reduce_empty_list_returns_undef() {
    assert_eq!(eval_int("defined(() |> preduce { $a + $b }) ? 1 : 0"), 0);
}

#[test]
fn parallel_reduce_string_concat() {
    assert_eq!(
        eval_string(r#"("a","b","c") |> preduce { $a . $b }"#),
        "abc"
    );
}

#[test]
fn parallel_reduce_with_array_variable() {
    assert_eq!(
        eval_int("my @nums = (10, 20, 30); @nums |> preduce { $a + $b }"),
        60
    );
}

#[test]
fn preduce_init_empty_returns_identity() {
    assert_eq!(eval_int("() |> preduce_init 0, { $a + $b }"), 0);
}

#[test]
fn preduce_init_single_element_folds_from_identity() {
    assert_eq!(eval_int("(9) |> preduce_init 0, { $a + $b }"), 9);
}

#[test]
fn preduce_init_histogram_merges_partials() {
    assert_eq!(
        eval_int(r#"my $h = ("a","b","a") |> preduce_init {}, { $a->{$b}++; $a }; $h->{a}"#),
        2
    );
    assert_eq!(
        eval_int(r#"my $h = ("a","b","a") |> preduce_init {}, { $a->{$b}++; $a }; $h->{b}"#),
        1
    );
    assert_eq!(
        eval_int(
            r#"my $h = ("x","y","x") |> preduce_init {}, { my ($acc, $item) = @_; $acc->{$item}++; $acc }; $h->{"x"}"#
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
    assert_eq!(eval_int(r#"(3, 5) |> preduce { $a + $b }"#), 8);
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
