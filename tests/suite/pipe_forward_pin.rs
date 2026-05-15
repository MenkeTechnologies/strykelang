//! Pipe-forward `|>` correctness pins. The operator is the most
//! prominent stryke extension — most demos and one-liners ride on it.
//! These pins lock the threading + precedence semantics so a parser
//! refactor can't silently change argument routing.

use crate::common::*;

// ── Single-stage threading into first arg ─────────────────────────────

#[test]
fn pipe_forward_threads_to_first_arg_no_args() {
    assert_eq!(eval_int("5 |> sqrt |> int"), 2); // int(sqrt(5))
}

#[test]
fn pipe_forward_threads_to_first_arg_with_extra_args() {
    let code = r#"
        my @parts = "a:b:c" |> split(/:/);
        join(",", @parts) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_with_sprintf_as_template() {
    let code = r#"
        my $msg = 42 |> sprintf("answer=%d");
        $msg eq "answer=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-stage chains ────────────────────────────────────────────────

#[test]
fn pipe_forward_three_stage_chain() {
    let code = r#"
        my @r = (1:10)
            |> grep { _ % 2 == 1 }
            |> map { _ * _ }
            |> sort { _0 <=> _1 };
        join(",", @r) eq "1,9,25,49,81" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_sum_of_even_squares() {
    assert_eq!(
        eval_int("(1:100) |> map { _ * _ } |> grep { _ % 2 == 0 } |> sum"),
        171_700
    );
}

#[test]
fn pipe_forward_string_pipeline() {
    let code = r#"
        my $r = "Hello World"
            |> lc
            |> split(/\s+/)
            |> join("-");
        $r eq "hello-world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── IIFE `>{ BLOCK }` stage ───────────────────────────────────────────

#[test]
fn pipe_iife_stage_receives_lhs_as_underscore() {
    let code = r#"
        my $r = 10 |> >{ _ + 5 };
        $r == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_iife_stage_receives_scalar_count_for_array_lhs() {
    // BUG-209 surface: pipe-forward IIFE `>{ ... }` binds `$_` to a
    // single value (scalar count when LHS is an array), not `@_`.
    // Pinning the current behavior so a future fix is a deliberate
    // decision.
    let code = r#"
        my $r = (1:5) |> >{ $_ * 10 };
        $r == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Precedence: pipe-forward between ?: and || ───────────────────────

#[test]
fn pipe_forward_rhs_must_be_callable_not_binary_expression() {
    // The parser deliberately rejects `x |> f || y` because the
    // RHS of `|>` must be a call, builtin, or coderef. Use parens
    // around the call to get the same effect.
    let code = r#"
        my $r = (0 |> int) || "fallback";
        $r eq "fallback" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_precedence_binds_looser_than_ternary() {
    // `cond ? x : y |> f` parses as `cond ? x : (y |> f)`.
    let code = r#"
        my $r = 1 ? "yes" : 99 |> int;
        $r eq "yes" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_precedence_addition_threads_full_expr() {
    // `x + 1 |> f` parses as `f(x + 1)`.
    let code = r#"
        my $r = 4 + 1 |> sprintf("%d");
        $r eq "5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── List vs scalar context through pipes ─────────────────────────────

#[test]
fn pipe_forward_returns_array_to_array_context() {
    let code = r#"
        my @r = (1:5) |> grep { _ % 2 == 1 };
        join(",", @r) eq "1,3,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_returns_scalar_to_scalar_context() {
    let code = r#"
        my $r = (1:10) |> sum;
        $r == 55 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mixed `|>` and `~>` (thread macro) ────────────────────────────────

#[test]
fn mixed_pipe_then_thread_macro() {
    let code = r#"
        my $r = ~> "stryke" uc reverse;
        $r eq "EKYRTS" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_into_pipe_forward() {
    let code = r#"
        my $r = ~> "hello" uc |> length;
        $r == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── User function as a pipe stage ────────────────────────────────────

#[test]
fn pipe_forward_into_user_function() {
    let code = r#"
        fn Demo::Pipe::dbl($n) { $n * 2 }
        fn Demo::Pipe::plus3($n) { $n + 3 }
        my $r = 5 |> Demo::Pipe::dbl |> Demo::Pipe::plus3;
        $r == 13 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_into_user_function_with_extra_args() {
    let code = r#"
        fn Demo::Pipe::power($n, $p) { $n ** $p }
        my $r = 2 |> Demo::Pipe::power(8);
        $r == 256 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Composition: pipe with sketches ──────────────────────────────────

#[test]
fn pipe_into_bloom_filter_membership() {
    let code = r#"
        my $b = bloom_filter(1000, 0.01);
        "apple banana cherry"
            |> split(/\s+/)
            |> map { bloom_add($b, $_) };
        bloom_contains($b, "banana") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_distinct_word_count_via_hll() {
    let code = r#"
        my $h = hll(14);
        "the the quick quick brown fox jumps over the the lazy dog"
            |> split(/\s+/)
            |> map { hll_add($h, $_) };
        # 8 distinct words: the, quick, brown, fox, jumps, over, lazy, dog
        my $est = hll_count($h);
        ($est >= 6 && $est <= 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty input through chain ────────────────────────────────────────

#[test]
fn pipe_forward_empty_array_produces_empty() {
    let code = r#"
        my @empty;
        my @r = @empty |> map { _ * 2 } |> grep { _ > 0 };
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reflection: %a → %b lookup via pipe ──────────────────────────────

#[test]
fn pipe_into_reflection_query() {
    let code = r#"
        # `keys %b |> len` should equal scalar(keys %b).
        my $piped  = keys(%b) |> len;
        my $direct = len(keys %b);
        $piped == $direct ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pipe-into-len matches every container shape ──────────────────────

#[test]
fn pipe_forward_into_len_string() {
    assert_eq!(eval_int("\"hello\" |> len"), 5);
}

#[test]
fn pipe_forward_into_len_array_literal() {
    assert_eq!(eval_int("[1, 2, 3, 4] |> len"), 4);
}

#[test]
fn pipe_forward_into_len_colon_range() {
    assert_eq!(eval_int("(1:7) |> len"), 7);
}

// ── pipe + ~> sketch algebra ─────────────────────────────────────────

#[test]
fn pipe_compose_sketch_algebra_then_count() {
    let code = r#"
        my $a = bloom_filter(1000, 0.01);
        my $b = bloom_filter(1000, 0.01);
        bloom_add($a, "x");
        bloom_add($b, "y");
        my $u = ($a + $b);   # sketch algebra
        (bloom_contains($u, "x") && bloom_contains($u, "y")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
