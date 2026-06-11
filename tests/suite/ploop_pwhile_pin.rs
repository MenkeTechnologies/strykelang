//! Pin `ploop` / `pwhile` / `pforeach` — the parallel counterparts of
//! `loop` / `while` / `foreach` (added 2026-06-11). `ploop [N] { … }`
//! and `pwhile [N] (COND) { … }` desugar at parse time to
//! `pfor { while (COND) { BODY } } 1..N` (N defaults to
//! `thread_count()`), so `_` is the 1-based worker id, `last` exits one
//! worker's loop, and the construct finishes when every worker exits.
//! `pforeach` is an interchangeable alias for `pfor`, same as
//! `for`/`foreach`.

use crate::common::*;

// ── ploop: parallel infinite loop ─────────────────────────────────────

#[test]
fn ploop_explicit_count_runs_one_body_per_worker() {
    let code = r#"
        mysync $n = 0;
        ploop 4 { $n += 1; last }
        $n
    "#;
    assert_eq!(eval_int(code), 4);
}

#[test]
fn ploop_binds_one_based_worker_id_to_topic() {
    let code = r#"
        mysync $sum = 0;
        ploop 3 { $sum += $_; last }
        $sum
    "#;
    assert_eq!(eval_int(code), 6); // 1 + 2 + 3
}

#[test]
fn ploop_default_count_is_thread_count() {
    let code = r#"
        mysync $n = 0;
        ploop { $n += 1; last }
        $n == thread_count() ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ploop_count_can_be_a_scalar_variable() {
    let code = r#"
        my $k = 5;
        mysync $n = 0;
        ploop $k { $n += 1; last }
        $n
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn ploop_body_iterates_until_last() {
    // One worker, `next` skips ahead, `last` exits after 5 iterations.
    let code = r#"
        mysync $i = 0;
        ploop 1 { $i += 1; next if $i < 5; last }
        $i
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn ploop_workers_push_to_shared_mysync_array() {
    let code = r#"
        mysync @done;
        ploop 4 { push @done, $_; last }
        len(@done)
    "#;
    assert_eq!(eval_int(code), 4);
}

// ── pwhile: parallel while ────────────────────────────────────────────

#[test]
fn pwhile_shared_condition_stops_all_workers() {
    let code = r#"
        mysync $n = 0;
        pwhile ($n < 100) { $n += 1 }
        $n >= 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pwhile_explicit_count_with_stop_flag() {
    let code = r#"
        mysync $run = 1;
        mysync $steps = 0;
        pwhile 4 ($run) { $steps += 1; $run = 0 if $steps >= 20 }
        $steps >= 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pwhile_count_can_be_a_scalar_variable() {
    let code = r#"
        my $w = 2;
        mysync $n = 0;
        pwhile $w ($n < 50) { $n += 1 }
        $n >= 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pwhile_false_condition_runs_zero_iterations() {
    let code = r#"
        mysync $n = 0;
        pwhile 4 (0) { $n += 1 }
        $n
    "#;
    assert_eq!(eval_int(code), 0);
}

// ── pforeach: alias for pfor ──────────────────────────────────────────

#[test]
fn pforeach_prefix_form_matches_pfor() {
    let code = r#"
        mysync @r;
        pforeach { push @r, $_ * 2 } 1:5;
        my @s = sort { $a <=> $b } @r;
        join(",", @s) eq "2,4,6,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pforeach_postfix_form_matches_pfor() {
    let code = r#"
        mysync @r;
        { push @r, $_ } pforeach 1:3;
        len(@r)
    "#;
    assert_eq!(eval_int(code), 3);
}
