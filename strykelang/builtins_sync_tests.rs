//! Tests for `mutex` and `semaphore` builtins (basic sync primitives).
//!
//! Covers: single-threaded state transitions, multi-threaded mutual
//! exclusion (mysync counter under N spawned threads), `try_*` semantics
//! when held/free, semaphore bounded-concurrency invariant.

use crate::run;

// ── Single-threaded semantic tests ───────────────────────────────────────

#[test]
fn mutex_initially_unlocked() {
    let code = r#"
        my $m = mutex();
        mutex_is_locked($m);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 0);
}

#[test]
fn mutex_lock_marks_held_unlock_clears() {
    let code = r#"
        my $m = mutex();
        mutex_lock($m);
        my $a = mutex_is_locked($m);
        mutex_unlock($m);
        my $b = mutex_is_locked($m);
        "$a,$b";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "1,0");
}

#[test]
fn mutex_try_lock_when_free_returns_one() {
    let code = r#"
        my $m = mutex();
        mutex_try_lock($m);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 1);
}

#[test]
fn mutex_try_lock_when_held_returns_zero() {
    let code = r#"
        my $m = mutex();
        mutex_lock($m);
        mutex_try_lock($m);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 0);
}

#[test]
fn mutex_try_lock_after_unlock_succeeds_again() {
    let code = r#"
        my $m = mutex();
        mutex_lock($m);
        my $a = mutex_try_lock($m);
        mutex_unlock($m);
        my $b = mutex_try_lock($m);
        "$a,$b";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "0,1");
}

// ── Semaphore single-threaded ────────────────────────────────────────────

#[test]
fn semaphore_permits_initial() {
    let code = r#"
        my $s = semaphore(5);
        semaphore_permits($s);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 5);
}

#[test]
fn semaphore_limit_returns_initial() {
    let code = r#"
        my $s = semaphore(8);
        semaphore_acquire($s);
        semaphore_acquire($s);
        semaphore_limit($s);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 8);
}

#[test]
fn semaphore_acquire_decrements_release_increments() {
    let code = r#"
        my $s = semaphore(3);
        semaphore_acquire($s);
        my $a = semaphore_permits($s);   # 2
        semaphore_acquire($s);
        my $b = semaphore_permits($s);   # 1
        semaphore_release($s);
        my $c = semaphore_permits($s);   # 2
        "$a,$b,$c";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "2,1,2");
}

#[test]
fn semaphore_try_acquire_drains_to_zero() {
    let code = r#"
        my $s = semaphore(5);
        my @r;
        push @r, semaphore_try_acquire($s) for (1:6);   # 5 ones then a zero
        join(',', @r);
    "#;
    assert_eq!(run(code).expect("run").to_string(), "1,1,1,1,1,0");
}

#[test]
fn semaphore_try_acquire_recovers_after_release() {
    let code = r#"
        my $s = semaphore(1);
        my $a = semaphore_try_acquire($s);   # 1
        my $b = semaphore_try_acquire($s);   # 0
        semaphore_release($s);
        my $c = semaphore_try_acquire($s);   # 1
        "$a,$b,$c";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "1,0,1");
}

#[test]
fn semaphore_aliases_resolve_same_builtin() {
    let code = r#"
        my $s = sem(2);
        sem_acquire($s);
        my $p = sem_permits($s);
        sem_release($s);
        my $q = sem_permits($s);
        "$p,$q";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "1,2");
}

#[test]
fn semaphore_zero_permits_try_acquire_fails() {
    let code = r#"
        my $s = semaphore(0);
        semaphore_try_acquire($s);
    "#;
    assert_eq!(run(code).expect("run").to_int(), 0);
}

#[test]
fn semaphore_negative_n_is_runtime_error() {
    let code = r#"
        eval { semaphore(-1) };
        $@ ne "" ? "err" : "ok";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "err");
}

// ── Type-mismatch errors ─────────────────────────────────────────────────

#[test]
fn mutex_lock_rejects_non_mutex() {
    let code = r#"
        eval { mutex_lock(42) };
        $@ =~ /must be a mutex/ ? "ok" : "fail:$@";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}

#[test]
fn semaphore_acquire_rejects_non_semaphore() {
    let code = r#"
        eval { semaphore_acquire("hi") };
        $@ =~ /must be a semaphore/ ? "ok" : "fail:$@";
    "#;
    assert_eq!(run(code).expect("run").to_string(), "ok");
}

// ── Multi-threaded: mutex serializes shared state ────────────────────────
//
// `mysync $counter` is already atomic on `$counter++`, so to verify the
// mutex actually serializes a non-atomic compound update we read+write
// across two separate statements inside the critical section. Without
// the mutex this would race; with the mutex the threads observe a
// consistent old/new pair on every iteration.

#[test]
fn mutex_serializes_compound_update_across_threads() {
    let code = r#"
        my $m = mutex();
        mysync $counter = 0;
        my @t;
        for (1:20) {
            push @t, async {
                mutex_lock($m);
                my $old = $counter;
                $counter = $old + 1;
                mutex_unlock($m);
            };
        }
        await($_) for @t;
        $counter;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 20);
}

#[test]
fn mutex_with_defer_releases_on_scope_exit() {
    // defer { mutex_unlock } — RAII idiom from the design spec.
    let code = r#"
        my $m = mutex();
        mysync $counter = 0;
        my @t;
        for (1:15) {
            push @t, async {
                mutex_lock($m);
                defer { mutex_unlock($m) }
                my $old = $counter;
                $counter = $old + 1;
            };
        }
        await($_) for @t;
        $counter;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 15);
}

// ── Multi-threaded: semaphore bounds concurrency ─────────────────────────
//
// Spawn N threads, each acquire→increment-in-flight→note-peak→sleep→
// decrement-in-flight→release. Verify the recorded peak never exceeded
// the semaphore limit. `mysync` makes the in-flight counter and peak
// atomic so the assertion itself is race-free.

#[test]
fn semaphore_bounds_concurrent_workers_to_limit() {
    let code = r#"
        my $s = semaphore(3);
        mysync $in_flight = 0;
        mysync $peak      = 0;
        my @t;
        for (1:10) {
            push @t, async {
                semaphore_acquire($s);
                $in_flight++;
                if ($in_flight > $peak) { $peak = $in_flight; }
                sleep(0.02);
                $in_flight--;
                semaphore_release($s);
            };
        }
        await($_) for @t;
        $peak;
    "#;
    let peak = run(code).expect("run").to_int();
    assert!(
        peak <= 3,
        "semaphore(3) should never let more than 3 workers in flight, got peak={peak}"
    );
    assert!(
        peak >= 1,
        "semaphore should let at least some work happen, got peak={peak}"
    );
}

#[test]
fn semaphore_one_acts_like_mutex() {
    // semaphore(1) is the textbook reduction to a mutex — verify it works
    // the same way for protecting a shared compound update.
    let code = r#"
        my $s = semaphore(1);
        mysync $counter = 0;
        my @t;
        for (1:25) {
            push @t, async {
                semaphore_acquire($s);
                my $old = $counter;
                $counter = $old + 1;
                semaphore_release($s);
            };
        }
        await($_) for @t;
        $counter;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 25);
}

// ── Reference semantics (Arc-shared across closure captures) ─────────────

#[test]
fn mutex_arc_shared_across_closure_captures() {
    // A mutex created outside an async block and captured inside must
    // refer to the SAME underlying handle — locking in one thread
    // blocks try_lock in another.
    let code = r#"
        my $m = mutex();
        mutex_lock($m);
        my $t = async {
            mutex_try_lock($m);   # must see it held — return 0
        };
        my $result = await($t);
        mutex_unlock($m);
        $result;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 0);
}

#[test]
fn semaphore_arc_shared_across_closure_captures() {
    let code = r#"
        my $s = semaphore(2);
        semaphore_acquire($s);
        semaphore_acquire($s);
        my $t = async {
            semaphore_try_acquire($s);   # no permits left → 0
        };
        my $r = await($t);
        semaphore_release($s);
        semaphore_release($s);
        $r;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 0);
}
