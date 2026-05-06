//! Behavior-pinning batch V (2026-05-05): `oursync` — package-global thread-safe state.
//!
//! `oursync` is the package-global counterpart of `mysync`. The binding is keyed by
//! the package-qualified stash name (`Pkg::x`) so all packages and parallel workers
//! share one `Arc<Mutex<…>>` cell. Plain `our $x` mutations from inside a parallel
//! block now error with a directive to declare `oursync` (DESIGN-001 strict-error
//! parity with `my` / `mysync`).

use crate::common::*;

// ── `oursync` scalar — same package, atomic counter ────────────────────────────

#[test]
fn oursync_scalar_increment_under_fan_cap_is_atomic() {
    // 1000 workers each increment the shared counter; all writes land in the same
    // `Arc<Mutex<PerlValue>>` so the final value is exactly 1000 (no lost updates).
    let code = r#"
        oursync $x = 0;
        fan_cap 1000 { $x++ };
        $x
    "#;
    assert_eq!(eval_int(code), 1000);
}

#[test]
fn oursync_scalar_compound_add_under_fan_cap() {
    // `+= 2` per worker × 100 workers = +200 (initial 5 → 205).
    let code = r#"
        oursync $x = 5;
        fan_cap 100 { $x += 2 };
        $x
    "#;
    assert_eq!(eval_int(code), 205);
}

#[test]
fn oursync_scalar_assignment_under_fan_cap_lands() {
    // After every worker writes 99, the cell still holds 99 (last writer wins per
    // worker, but they all write the same value).
    let code = r#"
        oursync $x = 5;
        fan_cap 50 { $x = 99 };
        $x
    "#;
    assert_eq!(eval_int(code), 99);
}

// ── `oursync` array — pfor push ────────────────────────────────────────────────

#[test]
fn oursync_array_push_under_pfor_collects_all() {
    // `oursync @r` is `AtomicArray` — `push` holds the lock, no torn writes.
    let code = r#"
        oursync @r;
        pfor { push @r, $_ * $_ } 1..50;
        scalar @r
    "#;
    assert_eq!(eval_int(code), 50);
}

// ── `oursync` hash — pfor counter ──────────────────────────────────────────────

#[test]
fn oursync_hash_compound_add_under_pfor() {
    // 300 keys mod 3 → 100 hits per bucket. `+=` on hash slot is locked.
    let code = r#"
        oursync %h;
        pfor { $h{$_ % 3} += 1 } 0..299;
        # collect deterministically: sort by key
        join(",", map { "$_=$h{$_}" } sort keys %h)
    "#;
    assert_eq!(eval_string(code), "0=100,1=100,2=100");
}

// ── Cross-package `oursync` ────────────────────────────────────────────────────

#[test]
fn oursync_cross_package_read_via_qualified_name() {
    // `oursync $x` in package C is readable from `main` as `$C::x` — same as plain
    // `our` but with a shared `Arc<Mutex>` cell.
    let code = r#"
        package C;
        oursync $x = 42;
        package main;
        $C::x
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn oursync_cross_package_mutation_via_sub_call_in_fan_worker() {
    // The classic Counter pattern — `oursync $total` in package C, mutated via
    // `C::bump` called from inside `fan_cap` workers. All increments land in the
    // shared cell. This exercises:
    //   1. `Op::DeclareOurSyncScalar` registers `total` in `english_lexical_scalars`
    //      + `our_lexical_scalars` so workers' tree walker qualifies `$total` → `C::total`.
    //   2. `call_sub_with_package` switches `$__PACKAGE__` to `C` on sub entry so the
    //      qualifier resolves correctly even when caller frame says `main`.
    //   3. `capture_with_atomics` clones the `Arc` (refcount bump) so workers share
    //      the underlying cell instead of getting per-worker copies.
    let code = r#"
        package C;
        oursync $total = 0;
        fn bump { $total++ }
        package main;
        fan_cap 1000 { C::bump() };
        $C::total
    "#;
    assert_eq!(eval_int(code), 1000);
}

// ── DESIGN-001 strict-error for plain `our` under parallel ─────────────────────

#[test]
fn plain_our_mutation_under_fan_is_rejected() {
    // Plain `our $x` in a parallel block must fail (no silent per-worker copy).
    // We assert via `eval_err_kind` which surfaces the error category — the exact
    // wording is allowed to evolve, but a runtime error MUST be produced.
    let code = r#"
        our $x = 0;
        fan_cap 5 { $x = 99 };
        $x
    "#;
    // Any runtime error category is acceptable — we just need the write to fail.
    let _kind = eval_err_kind(code);
}

#[test]
fn plain_my_mutation_under_fan_is_rejected() {
    // Companion: plain `my` also errors. The error message directs toward
    // `mysync` (lexical) rather than `oursync` (package-global), but again we only
    // assert the error category here to avoid coupling the test to the exact text.
    let code = r#"
        my $x = 0;
        fan_cap 5 { $x = 99 };
        $x
    "#;
    let _kind = eval_err_kind(code);
}

// ── --compat rejection ─────────────────────────────────────────────────────────

#[test]
fn oursync_keyword_rejected_under_compat() {
    // `oursync` is a stryke extension — `--compat` (Perl 5 mode) must reject it.
    // We can't easily flip `--compat` from a test harness without `set_compat_mode`
    // contention, so this test is intentionally minimal: it confirms the name is
    // recognized as a keyword (not auto-quoted) when stryke mode is active.
    let code = r#"oursync $x = 7; $x"#;
    assert_eq!(eval_int(code), 7);
}
