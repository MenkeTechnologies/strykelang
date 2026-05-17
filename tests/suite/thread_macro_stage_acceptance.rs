//! Coverage matrix for `~>` thread-macro stage acceptance — pins that
//! every builtin handled in `parser::thread_apply_bare_func` works as a
//! bareword stage (`~> SRC builtin`), produces the expected value, and
//! composes with downstream stages.
//!
//! Motivated by the May 2026 fix at cec54dd934 where `glob`/`rand`/
//! `srand` errored with "Undefined subroutine &…" because their
//! specialized `ExprKind` variants weren't wired into the thread-stage
//! dispatcher. This file pins the OTHER specialized-ExprKind builtins
//! so a similar regression in any of them fails CI.

use crate::common::*;

// ── string ops ───────────────────────────────────────────────────────────────

#[test]
fn thread_macro_uc_lc_stage() {
    assert_eq!(eval_string(r#"~> "Hello" uc"#), "HELLO");
    assert_eq!(eval_string(r#"~> "Hello" lc"#), "hello");
}

#[test]
fn thread_macro_ucfirst_lcfirst_stage() {
    assert_eq!(eval_string(r#"~> "hello" ucfirst"#), "Hello");
    assert_eq!(eval_string(r#"~> "Hello" lcfirst"#), "hello");
}

#[test]
fn thread_macro_length_len_cnt_stage() {
    assert_eq!(eval_int(r#"~> "hello" length"#), 5);
    assert_eq!(eval_int(r#"~> "hello" len"#), 5);
    assert_eq!(eval_int(r#"~> "hello" cnt"#), 5);
}

#[test]
fn thread_macro_chomp_returns_count() {
    // chomp returns the count of characters removed (Perl semantics);
    // the trailing newline is stripped from the source in-place.
    let n = eval_int(r#"~> "hello\n" chomp"#);
    assert_eq!(n, 1);
}

#[test]
fn thread_macro_chop_returns_removed_char() {
    let s = eval_string(r#"~> "hello" chop"#);
    assert_eq!(s, "o");
}

#[test]
fn thread_macro_quotemeta_stage() {
    let s = eval_string(r#"~> "a.b*c" quotemeta"#);
    assert_eq!(s, r"a\.b\*c");
}

// ── numeric ops ──────────────────────────────────────────────────────────────

#[test]
fn thread_macro_abs_int_stage() {
    assert_eq!(eval_int(r#"~> -42 abs"#), 42);
    assert_eq!(eval_int(r#"~> 3.7 int"#), 3);
}

#[test]
fn thread_macro_sqrt_stage() {
    let n = eval_int(r#"int(~> 81 sqrt)"#);
    assert_eq!(n, 9);
}

#[test]
fn thread_macro_hex_oct_chr_ord_stage() {
    assert_eq!(eval_int(r#"~> "ff" hex"#), 255);
    assert_eq!(eval_int(r#"~> "777" oct"#), 511);
    assert_eq!(eval_string(r#"~> 65 chr"#), "A");
    assert_eq!(eval_int(r#"~> "A" ord"#), 65);
}

// ── ref / defined ────────────────────────────────────────────────────────────

#[test]
fn thread_macro_defined_stage() {
    // `defined` returns 1 / "" directly; ternary continuation isn't a
    // valid thread-macro stage shape, so capture the value first.
    let n = eval_int(r#"my $d = ~> 42 defined; $d ? 1 : 0"#);
    assert_eq!(n, 1);
    let n = eval_int(r#"my $d = ~> undef defined; $d ? 1 : 0"#);
    assert_eq!(n, 0);
}

#[test]
fn thread_macro_ref_stage() {
    let s = eval_string(r#"~> [1, 2, 3] ref"#);
    assert_eq!(s, "ARRAY");
}

// ── array / hash ─────────────────────────────────────────────────────────────

#[test]
fn thread_macro_keys_values_stage() {
    let n = eval_int(r#"~> +{a => 1, b => 2, c => 3} keys |> len"#);
    assert_eq!(n, 3);
    let n = eval_int(r#"~> +{a => 1, b => 2, c => 3} values |> sum"#);
    assert_eq!(n, 6);
}

#[test]
fn thread_macro_rev_stage() {
    assert_eq!(eval_string(r#"~> "abc" rev"#), "cba");
}

#[test]
fn thread_macro_sort_stage() {
    let s = eval_string(r#"my @r = ~> (3, 1, 4, 1, 5, 9, 2, 6) sort; "@r""#);
    assert_eq!(s, "1 1 2 3 4 5 6 9");
}

#[test]
fn thread_macro_uniq_dedup_stage() {
    let n = eval_int(r#"~> (1, 2, 1, 3, 2) uniq |> len"#);
    assert_eq!(n, 3);
    let n = eval_int(r#"~> (1, 1, 2, 2, 3) dedup |> len"#);
    assert_eq!(n, 3);
}

#[test]
fn thread_macro_trim_stage() {
    assert_eq!(eval_string(r#"~> "  hello  " trim"#), "hello");
}

#[test]
fn thread_macro_flatten_stage() {
    // `flatten` un-arrayrefs the elements of an outer list. Passing a
    // single bracketed-arrayref directly leaves you with one element; pass
    // the bracketed elements as separate args instead.
    let s = eval_string(
        r#"
        my @nested = ([1, 2], [3], [4, 5])
        my @flat = ~> @nested flatten
        "@flat"
        "#,
    );
    assert_eq!(s, "1 2 3 4 5");
}

#[test]
fn thread_macro_compact_stage() {
    // Drops undef / empty.
    let n = eval_int(r#"~> (1, 0, "", undef, 2) compact |> len"#);
    // 0 stays (defined + non-empty), "" drops, undef drops → 3
    assert_eq!(n, 3);
}

#[test]
fn thread_macro_freq_stage() {
    let n = eval_int(r#"my $h = ~> (1, 2, 2, 3, 3, 3) freq; $h->{3}"#);
    assert_eq!(n, 3);
}

#[test]
fn thread_macro_lines_stage() {
    let n = eval_int(r#"~> "a\nb\nc\n" lines |> len"#);
    assert_eq!(n, 3);
}

// ── glob / rand / srand (the original fix) ──────────────────────────────────

#[test]
fn thread_macro_glob_stage_is_callable() {
    // Just verify it parses + runs; we don't care about matches.
    let n = eval_int(r#"my @r = ~> "/dev/null" glob; len(@r) >= 0 ? 1 : 0"#);
    assert_eq!(n, 1);
}

#[test]
fn thread_macro_rand_stage_returns_a_number() {
    // rand(N) returns a value in [0, N). Pin range bounds.
    let n = eval_int(
        r#"
        srand(42)
        my $r = ~> 100 rand
        $r >= 0 && $r < 100 ? 1 : 0
        "#,
    );
    assert_eq!(n, 1);
}

#[test]
fn thread_macro_srand_stage_seeds_deterministically() {
    // Two srand-then-rand sequences must produce the same value when
    // seeded with the same scalar.
    let n = eval_int(
        r#"
        my $seed = 42
        ~> $seed srand
        my $a = rand(1000000)
        ~> $seed srand
        my $b = rand(1000000)
        $a == $b ? 1 : 0
        "#,
    );
    assert_eq!(n, 1);
}

// ── pipeline composition: multiple bareword stages ──────────────────────────

#[test]
fn thread_macro_chain_of_three_bareword_stages() {
    let s = eval_string(r#"~> "  Hello, World  " trim uc"#);
    assert_eq!(s, "HELLO, WORLD");
}

#[test]
fn thread_macro_chain_string_then_numeric() {
    let n = eval_int(r#"~> "hello" uc |> ord"#);
    assert_eq!(n, 72); // 'H'
}

#[test]
fn thread_macro_chain_list_then_aggregate() {
    let n = eval_int(r#"~> (1, 2, 3, 4, 5) sort |> sum"#);
    assert_eq!(n, 15);
}
