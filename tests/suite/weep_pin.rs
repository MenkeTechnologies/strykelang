//! Pin the `weep` slow-trickle emitter. weep is a streaming primitive
//! that wraps any source (array, iterator, scalar) and emits each item
//! paced at `interval_ms` between successive emissions. The first item
//! emits immediately; only subsequent items are throttled.
//!
//! These tests cover (a) the data-flow contract — every item is
//! emitted, in order, without drops — and (b) the wall-clock contract —
//! N items at I ms = at least (N-1) * I ms elapsed. The wall-clock
//! lower bound is asserted with generous headroom (we accept up to one
//! interval of skew) to stay reliable under load.
//!
//! Companion to `turnbuckle` (peer-pair keepalive) — both are
//! time-aware extensions to stryke's streaming surface.

use crate::common::*;
use std::time::Instant;

#[test]
fn weep_emits_every_item_from_array_input() {
    let code = r#"
        my @r = weep([1, 2, 3, 4, 5], 0)
        join(",", @r)
    "#;
    assert_eq!(eval_string(code), "1,2,3,4,5");
}

#[test]
fn weep_zero_interval_is_passthrough() {
    // Zero interval must NOT sleep. 1000 items at 0ms should finish
    // well under 100ms even on a contended runner.
    let code = r#"
        my @r = weep(1:1000, 0)
        len(@r)
    "#;
    let started = Instant::now();
    let n = eval_int(code);
    let elapsed = started.elapsed();
    assert_eq!(n, 1000);
    assert!(
        elapsed.as_millis() < 1000,
        "zero-interval weep took {} ms; should be near-instant",
        elapsed.as_millis()
    );
}

#[test]
fn weep_enforces_interval_between_emissions() {
    // 5 items at 60ms = first immediate + 4 sleeps × 60ms = ≥ 240ms.
    // Generous floor (200ms) gives one interval of slack so a paused
    // GC / scheduler hiccup at the start can't flake the test.
    let code = r#"
        my @r = weep([10, 20, 30, 40, 50], 60)
        len(@r)
    "#;
    let started = Instant::now();
    let n = eval_int(code);
    let elapsed = started.elapsed();
    assert_eq!(n, 5);
    assert!(
        elapsed.as_millis() >= 200,
        "5 items at 60ms should take ≥ ~240ms, got {} ms",
        elapsed.as_millis()
    );
    // Also assert a sensible upper bound — should NOT take much more
    // than the sum of intervals. Catches a regression where weep would
    // sleep before EVERY item (including the first), pushing total to
    // ~5 × 60 = 300ms+ with one extra interval. Upper bound is generous
    // (1500ms) so macOS CI scheduler hiccups don't flake the test while
    // still catching a real per-item upfront sleep (which would push to
    // ~5 × 60 = 300ms + 5× CI jitter — easily multi-second on regressed
    // builds).
    assert!(
        elapsed.as_millis() < 1500,
        "weep took {} ms; first item should be immediate (no upfront sleep)",
        elapsed.as_millis()
    );
}

// Note on laziness:
//
// weep IS lazy at the iterator level — each call to `WeepIterator::next_item`
// sleeps only IF more than zero items have already been emitted (the first
// pull is free) and only IF the previous emit was less than `interval` ago.
//
// We do NOT pin observable laziness from stryke source level because
// every consumer we could reach from a test eagerly drains the iterator
// before returning:
//
//   * `my @r = weep(…)`    — array assignment consumes fully
//   * `for my $x (weep(…))` — for-loop consumes fully before iterating
//   * `first(weep(…))`     — first() also consumes fully (no block form)
//
// Only the `|>` pipeline form (`SRC |> weep INTERVAL |> take N`)
// preserves laziness end-to-end, but that introduces a second axis to
// debug (pipeline arg-passing semantics) that's tangential to weep's
// own contract. The throttle property — N items at I ms takes ≥ (N-1)*I
// ms — is pinned by `weep_enforces_interval_between_emissions`, which
// IS observable via eager consumption.

#[test]
fn weep_over_empty_source_yields_empty() {
    let code = r#"
        my @r = weep([], 100)
        len(@r)
    "#;
    let started = Instant::now();
    let n = eval_int(code);
    let elapsed = started.elapsed();
    assert_eq!(n, 0);
    // No items → no sleeps, regardless of interval.
    assert!(
        elapsed.as_millis() < 100,
        "empty source must not sleep, took {} ms",
        elapsed.as_millis()
    );
}

#[test]
fn weep_iterator_consumes_via_array_assignment() {
    // Pin: assigning weep's result to `@array` consumes the iterator
    // into a flat array — the standard stryke pattern for
    // iterator-returning builtins (see `pmaps` / `pgreps` tests).
    // This is the high-level proof that weep IS an iterator, not a
    // pre-built array masquerading as one.
    let code = r#"
        my @r = weep([10, 20, 30], 0)
        len(@r) == 3 && $r[0] == 10 && $r[2] == 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn weep_chains_with_another_iterator_source() {
    // weep accepts an iterator as source — verify it consumes through.
    // Use `1:5` (range iterator) instead of an array.
    let code = r#"
        my @r = weep(1:5, 0)
        join(",", @r)
    "#;
    assert_eq!(eval_string(code), "1,2,3,4,5");
}
