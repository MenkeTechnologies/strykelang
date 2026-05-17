//! Tests pinning the `par { BLOCK } LIST` top-level forms (prefix and
//! postfix), introduced 2026-05-16 alongside the existing `~>` macro-stage
//! shape. Pre-fix, calling `par` at top level emitted "Undefined subroutine
//! &par"; now it flows through the same `ExprKind::ParExpr` runtime path
//! as the macro stage and flattens to a list.

use crate::common::*;

// ── prefix form: par { BLOCK } LIST ──────────────────────────────────────────

#[test]
fn par_prefix_form_top_level_letters() {
    // Matches the docs example: per-chunk `letters` then flat concat.
    let s = eval_string(r#"my @a = par { letters } "Hello, World 123"; "@a""#);
    assert_eq!(s, "H e l l o W o r l d");
}

#[test]
fn par_prefix_form_top_level_length_matches_serial() {
    // Below the chunk threshold, par falls back to single-chunk eval; the
    // total length must match the serial `letters` builtin on the same input.
    let n = eval_int(r#"my @a = par { letters } "Hello, World 123"; len(@a)"#);
    assert_eq!(n, 10);
}

#[test]
fn par_prefix_form_returns_list_not_arrayref() {
    // `my @r = par { ... } LIST` should populate the array directly, not wrap
    // it in a single-element list-of-arrayref. Same convention as `letters`.
    let n = eval_int(r#"my @r = par { letters } "abc def"; len(@r)"#);
    assert_eq!(n, 6);
}

#[test]
fn par_prefix_form_chains_with_pipe() {
    // The top-level call yields a flat list, so a `|>` chain on it works the
    // same as any other list-yielding builtin.
    let n = eval_int(r#"par { letters } "Hello, World 123" |> len"#);
    assert_eq!(n, 10);
}

// ── postfix form: { BLOCK } par LIST  (statement-level only) ─────────────────

#[test]
fn par_postfix_form_runs_as_a_statement() {
    // Postfix `par` is statement-level (side-effecting): it runs the block on
    // each chunk for its effect, but the expression doesn't bind back. Verify
    // by mutating an outer accumulator.
    let n = eval_int(
        r#"
        my $count = 0
        { $count += len(_) } par "abc def"
        $count
        "#,
    );
    // Below the chunk threshold one chunk fires with the whole 7-char input.
    assert_eq!(n, 7);
}

// ── macro-stage form still works ─────────────────────────────────────────────

#[test]
fn par_macro_stage_unaffected_by_top_level_change() {
    // Regression guard: adding the prefix / postfix top-level shapes must not
    // break the original `~>` macro-stage path.
    let n = eval_int(r#"~> "abc def" par { letters } |> len"#);
    assert_eq!(n, 6);
}

// ── chunk binding: `_` and `_0` see the same value ───────────────────────────

#[test]
fn par_block_chunk_bound_to_topic_and_underscore_zero() {
    // Inside the `par` block, `_` and `_0` both alias the chunk. Use a length
    // assertion that's invariant under chunk subdivision.
    let n = eval_int(
        r#"
        my @r = par { my $x = len(_); my $y = len(_0); $x == $y ? (1) : (0) } "hello world"
        sum @r
        "#,
    );
    // 1 chunk below threshold, so we get exactly one truthy 1.
    assert_eq!(n, 1);
}
