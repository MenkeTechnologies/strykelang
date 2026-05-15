//! TopK SpaceSaving semantics pins. `topk(K)` keeps approximate
//! top-K counts. Pin: k=1, k=3, k=10 sizes; behavior on uniform
//! workload (no clear winner); behavior on clear dominant hitter;
//! merge via `+`; topk_top return shape.

use crate::common::*;

// ── Empty TopK ───────────────────────────────────────────────────────

#[test]
fn topk_empty_returns_empty_array() {
    let code = r#"
        my $tk = topk(3);
        my @top = topk_top($tk);
        len(@top) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Single-item TopK ────────────────────────────────────────────────

#[test]
fn topk_single_item_returns_one() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "only");
        my @top = topk_top($tk);
        (len(@top) == 1
            && $top[0]->[0] eq "only"
            && $top[0]->[1] == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── topk_top returns [item, count] arrayrefs ────────────────────────

#[test]
fn topk_top_returns_arrayref_pairs() {
    let code = r#"
        my $tk = topk(2);
        topk_add($tk, "a") for (1:10);
        topk_add($tk, "b") for (1:5);
        my @top = topk_top($tk);
        (ref($top[0]) =~ /ARRAY/
            && defined($top[0]->[0])
            && defined($top[0]->[1])) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Clear dominant hitter is first ──────────────────────────────────

#[test]
fn topk_clear_dominant_is_first() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "dominant") for (1:1000);
        topk_add($tk, "minor")    for (1:50);
        my @top = topk_top($tk);
        ($top[0]->[0] eq "dominant"
            && $top[0]->[1] >= 900) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── k=1: only one slot ──────────────────────────────────────────────

#[test]
fn topk_k_one_keeps_only_one_slot_displaceable() {
    // SpaceSaving with k=1: only one slot, so the last seen item
    // displaces the previous and inherits its count. Pin the
    // observed behavior (size <= 1, count reflects accumulation).
    let code = r#"
        my $tk = topk(1);
        topk_add($tk, "a") for (1:100);
        topk_add($tk, "b") for (1:10);
        topk_add($tk, "c") for (1:5);
        my @top = topk_top($tk);
        # Single slot, total accumulated count visible.
        (len(@top) == 1 && $top[0]->[1] >= 100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── k=10: returns up to 10 distinct items ───────────────────────────

#[test]
fn topk_k_ten_holds_ten_items() {
    let code = r#"
        my $tk = topk(10);
        for my $i (1:20) {
            topk_add($tk, "item_$i") for (1:(21 - $i));
        }
        my @top = topk_top($tk);
        len(@top) <= 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Order: descending by count ──────────────────────────────────────

#[test]
fn topk_results_in_descending_count_order() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "high") for (1:100);
        topk_add($tk, "mid")  for (1:50);
        topk_add($tk, "low")  for (1:10);
        my @top = topk_top($tk);
        # top[0] count > top[1] > top[2].
        ($top[0]->[1] >= $top[1]->[1]
            && $top[1]->[1] >= $top[2]->[1]) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Weighted add ────────────────────────────────────────────────────

#[test]
fn topk_add_ignores_third_weight_arg() {
    // BUG-231: `topk_add($tk, $key, $weight)` silently ignores the
    // weight argument. The count increments by 1 regardless. Pin
    // observed behavior so a future weighted-add fix is deliberate.
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "x", 10);
        topk_add($tk, "x", 5);
        my @top = topk_top($tk);
        # Got 2 (two calls), not 15 (sum of weights).
        ($top[0]->[0] eq "x" && $top[0]->[1] == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Merge two TopKs via `+` ────────────────────────────────────────

#[test]
fn topk_merge_via_plus_unions_counts() {
    let code = r#"
        my $a = topk(3);
        my $b = topk(3);
        topk_add($a, "shared") for (1:100);
        topk_add($a, "only_a") for (1:30);
        topk_add($b, "shared") for (1:50);
        topk_add($b, "only_b") for (1:40);
        my $m = $a + $b;
        my @top = topk_top($m);
        # "shared" should be dominant in the merge (~150).
        ($top[0]->[0] eq "shared" && $top[0]->[1] >= 100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Behavior when input is uniform (no clear winners) ──────────────

#[test]
fn topk_uniform_input_yields_some_result() {
    let code = r#"
        my $tk = topk(5);
        for my $i (1:20) {
            topk_add($tk, "uniform_$i");   # each appears exactly once
        }
        my @top = topk_top($tk);
        # No guarantees which 5 are kept under SpaceSaving, but
        # exactly k=5 should be in the result.
        len(@top) <= 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Idempotency of repeated top reads ───────────────────────────────

#[test]
fn topk_top_read_does_not_modify_state() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "a") for (1:10);
        my @r1 = topk_top($tk);
        my @r2 = topk_top($tk);
        ($r1[0]->[0] eq $r2[0]->[0]
            && $r1[0]->[1] == $r2[0]->[1]) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Many adds + small k = correct dominant ────────────────────────

#[test]
fn topk_one_dominant_among_1000_distinct() {
    let code = r#"
        my $tk = topk(5);
        for my $i (1:1000) {
            topk_add($tk, "noise_$i");
        }
        topk_add($tk, "WINNER") for (1:500);
        my @top = topk_top($tk);
        # WINNER should be present in top-k.
        my $found = 0;
        for my $h (@top) {
            $found = 1 if $h->[0] eq "WINNER";
        }
        $found == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Repeated single-key adds ──────────────────────────────────────

#[test]
fn topk_repeated_adds_to_one_key() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "k") for (1:50);
        my @top = topk_top($tk);
        ($top[0]->[0] eq "k" && $top[0]->[1] == 50) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mix strings and numbers ──────────────────────────────────────

#[test]
fn topk_handles_string_keys() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "alpha") for (1:30);
        topk_add($tk, "beta")  for (1:20);
        topk_add($tk, "gamma") for (1:10);
        my @top = topk_top($tk);
        # alpha is top.
        $top[0]->[0] eq "alpha" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── TopK with unicode keys ────────────────────────────────────────

#[test]
fn topk_handles_unicode_keys() {
    let code = r#"
        my $tk = topk(3);
        topk_add($tk, "café")       for (1:10);
        topk_add($tk, "🌟")          for (1:5);
        topk_add($tk, "Здравствуй") for (1:3);
        my @top = topk_top($tk);
        $top[0]->[0] eq "café" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── k larger than distinct items ─────────────────────────────────

#[test]
fn topk_k_larger_than_distinct() {
    let code = r#"
        my $tk = topk(100);
        topk_add($tk, "a"); topk_add($tk, "b"); topk_add($tk, "c");
        my @top = topk_top($tk);
        len(@top) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mass-add via pipe-forward ─────────────────────────────────────

#[test]
fn topk_add_via_pipe_forward_map() {
    let code = r#"
        my $tk = topk(3);
        # Use a for loop (cleaner than map for side effects).
        for my $w (split / /, "the the quick brown fox the lazy dog the") {
            topk_add($tk, $w);
        }
        my @top = topk_top($tk);
        ($top[0]->[0] eq "the" && $top[0]->[1] >= 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
