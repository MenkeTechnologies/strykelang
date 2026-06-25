//! Pin the syntactic-builtin alias reflection (`%a` / `%b` / `%all`).
//!
//! Builtins dispatched through the parser's `FuncCall` / `GrepExpr` arms
//! (`fi`, `nums`, `freq`, `distinct`, `fold`, …) never reach `try_builtin`, so
//! before the `SYNTACTIC_ALIASES` source they masqueraded as standalone `%b`
//! primaries (or were absent entirely — the `fi` bug). `build.rs` now reads the
//! alias→primary mapping straight off the dispatch `name:` literal and demotes
//! each alias spelling out of `%b` into `%a`.
//!
//! These tests pin both halves: (a) the reflection now classifies the alias
//! correctly, and (b) the alias still evaluates identically to its primary —
//! the eval half is the guard against the table ever asserting a falsehood.
//! Probed against the running interpreter on 2026-06-25.

use crate::common::*;

/// Every listed alias resolves to its primary in `%a` and is no longer a `%b`
/// primary. One assertion per pair keeps a regression's blame line precise.
fn assert_alias(alias: &str, primary: &str) {
    let code = format!(
        r#"(exists $a{{"{alias}"}} && $a{{"{alias}"}} eq "{primary}"
            && !(exists $b{{"{alias}"}})) ? 1 : 0"#
    );
    assert_eq!(eval_int(&code), 1, "{alias} should be %a alias of {primary}, not a %b primary");
}

#[test]
fn funccall_aliases_resolve_in_a_not_b() {
    // The spellings that were mis-classified as `%b` primaries before the fix.
    for (alias, primary) in [
        ("fi", "filter"),
        ("fold", "reduce"),
        ("inject", "reduce"),
        ("distinct", "uniq"),
        ("uq", "uniq"),
        ("freq", "frequencies"),
        ("frq", "frequencies"),
        ("nums", "numbers"),
        ("shuf", "shuffle"),
        ("sents", "sentences"),
        ("paras", "paragraphs"),
        ("cols", "columns"),
        ("grs", "graphemes"),
        ("punct", "punctuation"),
    ] {
        assert_alias(alias, primary);
    }
}

#[test]
fn block_keyword_short_aliases_resolve() {
    // gr/so/rd are the GrepExpr/SortExpr/ReduceExpr short forms wired through
    // the recognizer gates; `l` is the short form of `len`. All resolve in %a.
    for (alias, primary) in [("gr", "grep"), ("so", "sort"), ("rd", "reduce"), ("l", "len")] {
        assert_alias(alias, primary);
    }
}

#[test]
fn gr_so_rd_l_are_callable() {
    // Previously phantom — recognized in a dispatch arm but rejected at the
    // gate, so they errored `Undefined subroutine`. Now they evaluate.
    assert_eq!(
        eval_int(r#"my @g = gr { _ > 2 } (1, 2, 3, 4); (len(@g) == 2) ? 1 : 0"#),
        1,
        "gr should filter like grep"
    );
    assert_eq!(
        eval_int(r#"my @s = so { $a <=> $b } (3, 1, 2); ($s[0] == 1 && $s[2] == 3) ? 1 : 0"#),
        1,
        "so should sort like sort"
    );
    assert_eq!(
        eval_int(r#"((1:5 |> rd { $a + $b }) == 15) ? 1 : 0"#),
        1,
        "rd should reduce like reduce"
    );
    assert_eq!(
        eval_int(r#"my @a = (10, 20, 30); (l(@a) == 3 && l(@a) == len(@a)) ? 1 : 0"#),
        1,
        "l should count like len"
    );
}

#[test]
fn primaries_stay_primaries() {
    // The canonical targets must remain `%b` primaries (and out of `%a`).
    let code = r#"
        my @prim = ("filter", "reduce", "uniq", "frequencies", "numbers", "shuffle");
        my $ok = 1;
        for my $p (@prim) {
            $ok = 0 unless (exists $b{$p} && !(exists $a{$p}));
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn new_primaries_registered() {
    // `greps` (lazy grep) and `par` (parallel foreach) were callable but
    // absent from every reflection table; now registered as `%b` primaries.
    let code = r#"(exists $b{"greps"} && exists $b{"par"}
        && exists $all{"greps"} && exists $all{"par"}) ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn count_cluster_left_unchanged() {
    // The contested `len`/`count`/`cnt`/`size` synonym cluster is deliberately
    // excluded from remodeling — all four stay `%b` primaries.
    let code = r#"
        my @c = ("len", "count", "cnt", "size");
        my $ok = 1;
        for my $n (@c) { $ok = 0 unless (exists $b{$n} && !(exists $a{$n})); }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn all_equals_a_plus_b_plus_k() {
    // The partition invariant must survive the move of spellings from %b to %a.
    let code = r#"(len(keys %all) == len(keys %a) + len(keys %b) + len(keys %k)) ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn a_and_b_are_disjoint() {
    let code = r#"
        my $overlap = 0;
        for my $k (keys %a) { $overlap++ if exists $b{$k}; }
        $overlap == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── eval-equivalence: alias evaluates identically to its primary ──────────

#[test]
fn nums_evaluates_like_numbers() {
    let code = r#"
        my $a = ("a1 b22 c333" |> nums |> join ",");
        my $b = ("a1 b22 c333" |> numbers |> join ",");
        ($a eq $b) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn distinct_evaluates_like_uniq() {
    let code = r#"
        my @in = (1, 1, 2, 3, 3, 3);
        my $a = (@in |> distinct |> join ",");
        my $b = (@in |> uniq |> join ",");
        ($a eq $b) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn fold_evaluates_like_reduce() {
    let code = r#"
        ((1:5 |> fold { $a + $b }) == (1:5 |> reduce { $a + $b })) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn fi_evaluates_like_filter() {
    let code = r#"
        my $a = (1:9 |> fi { _ % 2 } |> collect |> join ",");
        my $b = (1:9 |> filter { _ % 2 } |> collect |> join ",");
        ($a eq $b) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
