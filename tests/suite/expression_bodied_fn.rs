//! Runtime tests for the expression-bodied `fn` form:
//!
//!     fn NAME(PARAMS) = EXPR
//!
//! Pre-existing parse pins live in `regression_2026_05_at.rs` and
//! `behavior_pin_2026_05_*`; this file pins RUNTIME semantics — that the
//! body really evaluates `EXPR` and returns its value, that recursion
//! resolves, that pipe-forward and thread-macro bodies compose, and that
//! lexical captures + default params behave as documented.

use crate::common::*;

// ── basic one-line bodies ────────────────────────────────────────────────────

#[test]
fn expr_body_returns_arithmetic_result() {
    let n = eval_int(r#"fn Demo::sq($n) = $n * $n; Demo::sq(7)"#);
    assert_eq!(n, 49);
}

#[test]
fn expr_body_returns_string() {
    let s = eval_string(r#"fn Demo::greet($who) = "Hello, $who!"; Demo::greet("world")"#);
    assert_eq!(s, "Hello, world!");
}

#[test]
fn expr_body_multi_param() {
    let n = eval_int(r#"fn Demo::add3($x, $y, $z) = $x + $y + $z; Demo::add3(10, 20, 30)"#);
    assert_eq!(n, 60);
}

#[test]
fn expr_body_with_ternary() {
    let n = eval_int(
        r#"
        fn Demo::abs_int($n) = $n < 0 ? -$n : $n
        Demo::abs_int(-13) + Demo::abs_int(7)
        "#,
    );
    assert_eq!(n, 20);
}

// ── recursion via expression body ───────────────────────────────────────────

#[test]
fn expr_body_recursive_factorial() {
    let n = eval_int(
        r#"
        fn Demo::fact($n) = $n <= 1 ? 1 : $n * Demo::fact($n - 1)
        Demo::fact(7)
        "#,
    );
    assert_eq!(n, 5040);
}

#[test]
fn expr_body_recursive_fibonacci() {
    let n = eval_int(
        r#"
        fn Demo::fib($n) = $n < 2 ? $n : Demo::fib($n - 1) + Demo::fib($n - 2)
        Demo::fib(15)
        "#,
    );
    assert_eq!(n, 610);
}

#[test]
fn expr_body_mutual_recursion() {
    let n = eval_int(
        r#"
        fn Demo::is_even($n) = $n == 0 ? 1 : Demo::is_odd($n - 1)
        fn Demo::is_odd($n)  = $n == 0 ? 0 : Demo::is_even($n - 1)
        Demo::is_even(10) + Demo::is_odd(7)
        "#,
    );
    assert_eq!(n, 2);
}

// ── pipe-forward & thread-macro inside expression body ─────────────────────

#[test]
fn expr_body_with_pipe_forward_chain() {
    let s = eval_string(
        r#"
        fn Demo::clean($s) = $s |> trim |> lc
        Demo::clean("  HeLLo  ")
        "#,
    );
    assert_eq!(s, "hello");
}

#[test]
fn expr_body_with_thread_macro() {
    let n = eval_int(
        r#"
        fn Demo::int_sqrt($n) = int(~> $n sqrt)
        Demo::int_sqrt(81) + Demo::int_sqrt(64)
        "#,
    );
    // 9 + 8 = 17
    assert_eq!(n, 17);
}

#[test]
fn expr_body_returning_list_via_pipe() {
    let n = eval_int(
        r#"
        fn Demo::word_count($s) = $s |> trim |> lines |> len
        Demo::word_count("a\nb\nc\n")
        "#,
    );
    assert_eq!(n, 3);
}

// ── closure capture ────────────────────────────────────────────────────────

#[test]
fn expr_body_captures_outer_lexical() {
    let n = eval_int(
        r#"
        my $factor = 10
        fn Demo::scale($n) = $n * $factor
        Demo::scale(5) + Demo::scale(7)
        "#,
    );
    // 50 + 70 = 120
    assert_eq!(n, 120);
}

// ── composability with `map` / `grep` / `~>` ───────────────────────────────

#[test]
fn expr_body_as_map_target() {
    let s = eval_string(
        r#"
        fn Demo::twice($n) = $n * 2
        my @r = map { Demo::twice(_) } 1, 2, 3, 4, 5
        "@r"
        "#,
    );
    assert_eq!(s, "2 4 6 8 10");
}

#[test]
fn expr_body_as_grep_predicate() {
    let s = eval_string(
        r#"
        fn Demo::is_big($n) = $n > 50
        my @r = grep { Demo::is_big(_) } (10, 60, 30, 80, 40, 90)
        "@r"
        "#,
    );
    assert_eq!(s, "60 80 90");
}

#[test]
fn expr_body_called_from_thread_macro_stage_block() {
    let n = eval_int(
        r#"
        fn Demo::cube($n) = $n * $n * $n
        ~> 1:5 map { Demo::cube(_) } |> sum
        "#,
    );
    // 1 + 8 + 27 + 64 + 125 = 225
    assert_eq!(n, 225);
}
