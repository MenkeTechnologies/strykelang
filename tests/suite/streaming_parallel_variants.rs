//! Runtime tests for the streaming parallel builtins: `pmaps`, `pgreps`,
//! `pflat_maps`. These are lazy iterators backed by background worker
//! threads — order is NOT deterministic (results arrive in completion
//! order, not input order), so all assertions here are order-insensitive
//! (set / sum / count).
//!
//! Contrast with `pmap` / `pgrep` / `pflat_map` (no trailing-s) which
//! preserve input order via eager collect + re-sort. See memory note
//! `project_streaming_parallel.md`.

use crate::common::*;

// ── pmaps: streaming parallel map ───────────────────────────────────────────

#[test]
fn pmaps_count_matches_input_count() {
    let n = eval_int(r#"my @r = pmaps { _ * 2 } 1, 2, 3, 4, 5; len(@r)"#);
    assert_eq!(n, 5);
}

#[test]
fn pmaps_sum_equals_serial_map_sum() {
    let n = eval_int(r#"sum pmaps { _ * 2 } 1, 2, 3, 4, 5"#);
    assert_eq!(n, 30); // 2+4+6+8+10
}

#[test]
fn pmaps_set_equality_with_serial_map() {
    // Pin set-equality (sorted), not order — pmaps is non-deterministic.
    let s = eval_string(
        r#"
        my @r = pmaps { _ * _ } 1, 2, 3, 4, 5
        my @sorted = sort { _0 <=> _1 } @r
        "@sorted"
        "#,
    );
    assert_eq!(s, "1 4 9 16 25");
}

#[test]
fn pmaps_empty_input_yields_empty_output() {
    let n = eval_int(r#"my @r = pmaps { _ * 2 } (); len(@r)"#);
    assert_eq!(n, 0);
}

#[test]
fn pmaps_one_element_yields_one_result() {
    let n = eval_int(r#"my @r = pmaps { _ + 100 } 42; $r[0]"#);
    assert_eq!(n, 142);
}

#[test]
fn pmaps_with_large_input_preserves_aggregate() {
    let n = eval_int(
        r#"
        my @big = 1:1000
        sum pmaps { _ } @big
        "#,
    );
    // 1000*1001/2 = 500500
    assert_eq!(n, 500500);
}

// ── pgreps: streaming parallel grep ────────────────────────────────────────

#[test]
fn pgreps_filter_count_matches_serial() {
    let n = eval_int(r#"len pgreps { _ > 50 } (1:100)"#);
    assert_eq!(n, 50);
}

#[test]
fn pgreps_predicate_filters_correctly_via_set_equality() {
    let s = eval_string(
        r#"
        my @evens = pgreps { _ % 2 == 0 } 1:10
        my @sorted = sort { _0 <=> _1 } @evens
        "@sorted"
        "#,
    );
    assert_eq!(s, "2 4 6 8 10");
}

#[test]
fn pgreps_all_match_returns_all() {
    let n = eval_int(r#"len pgreps { 1 } (1:20)"#);
    assert_eq!(n, 20);
}

#[test]
fn pgreps_none_match_returns_empty() {
    let n = eval_int(r#"len pgreps { 0 } (1:20)"#);
    assert_eq!(n, 0);
}

// ── pflat_maps: streaming parallel flat-map ────────────────────────────────

#[test]
fn pflat_maps_emits_n_items_per_input() {
    let n = eval_int(r#"len pflat_maps { (_, _ * 10) } 1, 2, 3, 4, 5"#);
    // 2 outputs per input × 5 inputs = 10
    assert_eq!(n, 10);
}

#[test]
fn pflat_maps_sum_aggregates_all_outputs() {
    let n = eval_int(r#"sum pflat_maps { (_, _ * 10) } 1, 2, 3"#);
    // (1+10) + (2+20) + (3+30) = 66
    assert_eq!(n, 66);
}

#[test]
fn pflat_maps_emits_more_items_than_inputs() {
    // 1-to-N flat-map: each input expands into multiple outputs.
    let n = eval_int(r#"len pflat_maps { (_, _ + 1, _ + 2) } 1, 2, 3"#);
    // 3 outputs × 3 inputs = 9
    assert_eq!(n, 9);
}

#[test]
fn pflat_maps_can_emit_zero_per_input_for_some() {
    let n = eval_int(
        r#"
        my @r = pflat_maps { _ % 2 == 0 ? (_) : () } 1:10
        len(@r)
        "#,
    );
    // 5 evens in 1..10
    assert_eq!(n, 5);
}

// ── streaming chains with downstream stage ──────────────────────────────────

#[test]
fn pmaps_then_sum_via_pipe() {
    let n = eval_int(r#"pmaps { _ * 3 } 1:10 |> sum"#);
    // 3 * (1+2+...+10) = 3 * 55 = 165
    assert_eq!(n, 165);
}

#[test]
fn pgreps_then_sort_then_take() {
    let s = eval_string(
        r#"
        my @big = pgreps { _ > 50 } (1:100)
        my @top3 = (sort { _1 <=> _0 } @big)[0:2]
        "@top3"
        "#,
    );
    assert_eq!(s, "100 99 98");
}

#[test]
fn pflat_maps_then_uniq_then_count() {
    let n = eval_int(
        r#"
        my @r = pflat_maps { (_ % 3, _ % 5) } 1:20
        my @u = uniq(@r)
        len(@u)
        "#,
    );
    // _ % 3 ∈ {0,1,2}, _ % 5 ∈ {0,1,2,3,4}. Union = {0,1,2,3,4} → 5
    assert_eq!(n, 5);
}
