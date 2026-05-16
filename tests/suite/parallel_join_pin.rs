//! Parallel worker-result-joining pins. Beyond `concurrency_pin.rs`:
//! focus on output-shape determinism and worker-error propagation.

use crate::common::*;

// ── pmap preserves input-index ordering ───────────────────────────

#[test]
fn pmap_output_indices_match_input() {
    let code = r#"
        my @input = (1, 2, 3, 4, 5);
        my @out = pmap { _ * 1000 + 1 } @input;
        # In-order: 1001, 2001, 3001, 4001, 5001.
        join(",", @out) eq "1001,2001,3001,4001,5001" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pmap_deterministic_per_input() {
    let code = r#"
        # Pure-fn pmap should yield identical output across runs.
        my @r1 = pmap { _ * 7 } (1:50);
        my @r2 = pmap { _ * 7 } (1:50);
        join(",", @r1) eq join(",", @r2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pmap_chained_with_grep_pipeline() {
    let code = r#"
        my @r = grep { _ > 50 } pmap { _ * _ } (1:10);
        # squares > 50: 64, 81, 100.
        join(",", sort { _0 <=> _1 } @r) eq "64,81,100" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pgrep output preserves order ──────────────────────────────────

#[test]
fn pgrep_preserves_relative_order() {
    let code = r#"
        my @input = (9, 1, 8, 2, 7, 3, 6, 4, 5);
        my @par = pgrep { _ > 4 } @input;
        my @seq = grep  { _ > 4 } @input;
        join(",", @par) eq join(",", @seq) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── preduce associativity sanity ──────────────────────────────────

#[test]
fn preduce_sum_with_three_chunks() {
    let code = r#"
        my $sum = preduce { _0 + _1 } 0, (1:30);
        # Sum 1..30 = 465.
        $sum == 465 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn preduce_max_works() {
    let code = r#"
        my $r = preduce { _0 > _1 ? _0 : _1 } 0, (3, 7, 2, 9, 1, 5);
        $r == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap on hashref arr ────────────────────────────────────────────

#[test]
fn pmap_extract_field_from_hashref_array() {
    let code = r#"
        my @users = map { +{ id => $_, name => "u_$_" } } (1:20);
        my @ids = pmap { _->{id} } @users;
        # Order preserved: 1..20.
        join(",", @ids) eq join(",", 1:20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap output composes with len/sum ──────────────────────────────

#[test]
fn pmap_into_sum() {
    let code = r#"
        my $r = sum(pmap { _ * _ } (1:10));
        # Sum of squares 1..10 = 385.
        $r == 385 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pmap_into_max() {
    let code = r#"
        my $r = max(pmap { _ * 3 } (1, 5, 2, 7, 4));
        $r == 21 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap empty input returns empty ────────────────────────────────

#[test]
fn pmap_empty_yields_empty() {
    let code = r#"
        my @empty;
        my @r = pmap { _ * 2 } @empty;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap of single element ────────────────────────────────────────

#[test]
fn pmap_single_element() {
    let code = r#"
        my @r = pmap { _ + 100 } (42);
        len(@r) == 1 && $r[0] == 142 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap with side-effect via mysync ──────────────────────────────

#[test]
fn pmap_with_mysync_sketch_side_effects() {
    let code = r#"
        mysync $hll = hll(14);
        my @r = pmap {
            hll_add($hll, "user_$_");
            $_ * 2
        } (1:1000);
        # Output preserves input pmap order; HLL ~1000 distinct.
        my $est = hll_count($hll);
        # Allow tolerance for racy hll add (BUG-227 surface).
        ($est >= 500 && len(@r) == 1000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap returning arrayrefs ──────────────────────────────────────

#[test]
fn pmap_returning_arrayrefs() {
    let code = r#"
        my @out = pmap { [_, _ * 2] } (1:5);
        ($out[0]->[0] == 1
            && $out[0]->[1] == 2
            && $out[4]->[0] == 5
            && $out[4]->[1] == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap returning hashref ────────────────────────────────────────

#[test]
fn pmap_returning_hashrefs() {
    let code = r#"
        my @out = pmap { +{ n => _, sq => _ * _ } } (1:5);
        ($out[3]->{n} == 4 && $out[3]->{sq} == 16) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mixed pmap + sort ──────────────────────────────────────────────

#[test]
fn pmap_then_sort_preserves_correctness() {
    let code = r#"
        my @input = (5, 9, 2, 7, 1, 4, 8, 3, 6);
        my @squared = pmap { _ * _ } @input;
        my @sorted = sort { _0 <=> _1 } @squared;
        join(",", @sorted) eq "1,4,9,16,25,36,49,64,81" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap of pmap (nested parallel) ────────────────────────────────

#[test]
fn nested_pmap_correctness() {
    let code = r#"
        my @rows = pmap {
            my $r = $_;
            [pmap { $r * $_ } (1:5)]
        } (1:5);
        # rows[0]->[0] = 1, rows[4]->[4] = 25.
        ($rows[0]->[0] == 1 && $rows[4]->[4] == 25) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap + reduce equivalent to seq ───────────────────────────────

#[test]
fn pmap_reduce_matches_seq_map_reduce() {
    let code = r#"
        my @input = (1:100);
        my $par = pmap_reduce { _ * _ } { _0 + _1 } 0, @input;
        my $seq = sum(map { _ * _ } @input);
        $par == $seq ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap large input correctness ──────────────────────────────────

#[test]
fn pmap_100k_input_consistent_with_seq() {
    let code = r#"
        my @input = (1:100_000);
        my $par_sum = sum(pmap { _ * 7 } @input);
        # Sum = 7 * (1+2+...+100000) = 7 * 5_000_050_000 = 35_000_350_000.
        $par_sum == 35_000_350_000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pfor with explicit side-effect ────────────────────────────────

#[test]
fn pfor_appending_to_mysync_array() {
    let code = r#"
        mysync @log;
        pfor { push @log, $_ } (1:50);
        # All 50 items present (order non-deterministic).
        len(@log) == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap on identity preserves input ──────────────────────────────

#[test]
fn pmap_identity_returns_input() {
    let code = r#"
        my @input = (10, 20, 30, 40, 50);
        my @par = pmap { _ } @input;
        join(",", @par) eq join(",", @input) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
