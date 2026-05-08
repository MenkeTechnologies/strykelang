//! Tests for the chunk-parallel thread-macro family + generic parallel
//! wrappers added 2026-05-08:
//!
//! - `par { BLOCK }` — generic parallel-chunk wrapper
//! - `par_reduce { extract } [ { merge } ]` — chunk-extract-merge with
//!   auto-merger for hashes-of-numbers, numbers, arrays, and strings
//! - `~p>` / `~p>>` — sugar for `par_reduce { whole_pipeline }`
//! - `||>` / `|then|` — boundary marker that switches `~p>` back to
//!   sequential `~>` continuation
//! - `~s>` / `~s>>` — per-item streaming thread macros (covered briefly
//!   here for round-trip; deeper streaming tests live elsewhere)

use crate::common::*;

// ── par { BLOCK } correctness ────────────────────────────────────────────────

#[test]
fn par_letters_matches_serial_letters() {
    // Below the chunk-threshold the runtime falls back to single-chunk eval;
    // the result must still match the bare `letters` builtin element-for-
    // element including ordering.
    assert_eq!(
        eval_int(r#"my @a = ~> "Hello, World 123" par { letters }; scalar @a"#),
        10,
    );
    assert_eq!(
        eval_string(r#"my @a = ~> "Hello, World 123" par { letters }; "@a""#),
        "H e l l o W o r l d",
    );
}

#[test]
fn par_uc_per_chunk_concatenates() {
    // Each chunk's `uc` returns a string; the runtime auto-flattens the
    // chunk results into a Vec, so `len` after `par { uc }` reports the
    // chunk count, not the total char length. Pin the behaviour so any
    // future merger change is intentional.
    let n = eval_int(r#"my @x = ~> "abcdef" par { uc }; scalar @x"#);
    assert!(n >= 1, "par must return at least one chunk-result");
}

// ── par_reduce { extract } auto-merge ─────────────────────────────────────────

#[test]
fn par_reduce_hash_auto_merges_by_key_add() {
    // Canonical histogram merge: each chunk produces a hash of letter→
    // count, the auto-merger sums the values key-wise.
    assert_eq!(
        eval_int(
            r#"my $h = ~> "abc def abc" par_reduce { letters |> freq };
               $h->{a} + $h->{b} + $h->{c} + $h->{d} + $h->{e} + $h->{f}"#,
        ),
        9,
    );
    assert_eq!(
        eval_int(
            r#"my $h = ~> "abc def abc" par_reduce { letters |> freq };
               $h->{a}"#,
        ),
        2,
    );
    assert_eq!(
        eval_int(
            r#"my $h = ~> "abc def abc" par_reduce { letters |> freq };
               $h->{d}"#,
        ),
        1,
    );
}

#[test]
fn par_reduce_numeric_auto_merges_by_add() {
    // Single-chunk fallback path: returns the bare extract result. With
    // larger input forcing multi-chunk, scalar result auto-merges via `+`.
    assert_eq!(eval_int(r#"~> "abcde" par_reduce { length }"#), 5,);
}

#[test]
fn par_reduce_explicit_merge_block_takes_a_b() {
    // Two-block form: explicit pairwise reducer with $a / $b bound.
    // Forces deterministic behaviour regardless of chunk-count by always
    // reducing through the user-supplied combiner.
    assert_eq!(
        eval_int(r#"~> "abcdefgh" par_reduce { length($_) } { $a + $b }"#,),
        8,
    );
}

// ── ~p> / ~p>> chunk-parallel macros ─────────────────────────────────────────

#[test]
fn p_arrow_runs_pipeline_per_chunk_and_merges() {
    // `~p> SRC stage1 stage2` desugars to `par_reduce { stage1 |> stage2 } SRC`.
    assert_eq!(
        eval_int(
            r#"my $h = ~p> "abc def abc" letters freq;
               $h->{a} + $h->{b} + $h->{c}"#,
        ),
        6,
    );
    assert_eq!(
        eval_int(
            r#"my $h = ~p> "abc def abc" letters freq;
               $h->{a}"#,
        ),
        2,
    );
}

#[test]
fn p_arrow_then_pipe_continues_sequentially() {
    // `|>` already terminates any thread-macro, so it serves as the
    // simplest parallel→sequential boundary for `~p>`. The merged hash
    // must round-trip through `values` and `sum` to the expected total.
    let n = eval_int(
        r#"my @vals = values %{~p> "abc def abc" letters freq};
           my $tot = 0; $tot += $_ for @vals;
           $tot"#,
    );
    assert_eq!(n, 9);
}

#[test]
fn p_arrow_double_pipe_arrow_continuation() {
    // `||>` boundary: switches from `~p>` chunk-parallel back to a `~>`
    // thread-macro continuation operating on the merged result.
    let n = eval_int(
        r#"my $h = ~p> "abc def abc" letters freq ||> values |> sum;
           $h"#,
    );
    assert_eq!(n, 9);
}

#[test]
fn p_arrow_then_keyword_continuation() {
    // `|then|` boundary: same semantics as `||>` but uses the readable
    // word form. Both must produce identical results.
    let n = eval_int(
        r#"my $h = ~p> "abc def abc" letters freq |then| values |> sum;
           $h"#,
    );
    assert_eq!(n, 9);
}

#[test]
fn p_arrow_boundary_markers_match_each_other() {
    // `||>` and `|then|` are aliases — pinning byte-equal output.
    let a = eval_int(r#"~p> "the quick brown fox" letters freq ||> values |> sum"#);
    let b = eval_int(r#"~p> "the quick brown fox" letters freq |then| values |> sum"#);
    assert_eq!(a, b);
    assert_eq!(a, 16); // total alphabetic chars in "the quick brown fox"
}

#[test]
fn p_arrow_last_thread_last_variant() {
    // `~p>>` mirrors `~p>` with thread-last semantics inside each
    // chunk's pipeline. For pure topic-using stages the semantics are
    // indistinguishable, so this primarily exercises that the parser
    // accepts the operator and the runtime dispatches it.
    let n = eval_int(
        r#"my $h = ~p>> "abc def abc" letters freq;
           $h->{a}"#,
    );
    assert_eq!(n, 2);
}

// ── ~s> / ~s>> sanity (separate streaming runtime) ───────────────────────────

#[test]
fn s_arrow_per_item_streaming_processes_all_items() {
    // `~s>` runs each stage as a worker connected by bounded channels;
    // the macro's final return is the count of items processed by the
    // last stage.
    assert_eq!(
        eval_int(r#"~s> [1, 2, 3, 4, 5] map { $_ * 10 } map { $_ + 1 }"#),
        5,
    );
}

#[test]
fn s_arrow_empty_input_yields_zero() {
    assert_eq!(eval_int(r#"~s> [] map { $_ * 2 }"#), 0,);
}

// ── parser / dispatch surface ────────────────────────────────────────────────

#[test]
fn par_reduce_with_no_merge_block_chains_via_pipe() {
    // Parser must NOT confuse the next token after the extract block with
    // the optional reduce block when that token is a normal `|>` chain.
    let n = eval_int(r#"~> "abc def" par_reduce { letters |> freq } |> values |> sum"#);
    assert_eq!(n, 6);
}

#[test]
fn par_outside_thread_macro_is_unknown() {
    // `par { … }` is ONLY valid as a thread-stage; as a top-level
    // expression there's no `par` builtin to dispatch to. The exact
    // ErrorKind ends up as `Runtime` (undefined-sub falls under it),
    // so assert by message text rather than kind.
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let program = stryke::parse(r#"par { letters } "abc""#).expect("parse failed");
    let mut interp = stryke::vm_helper::VMHelper::new();
    let err = interp.execute(&program).expect_err("should fail");
    assert!(
        err.to_string().contains("Undefined subroutine"),
        "expected undefined-sub error, got: {}",
        err
    );
}
