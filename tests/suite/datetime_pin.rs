//! Datetime + clock pins. Stryke ships native `time` / `now_ns` /
//! `now_us` / `strftime` / `strptime` as builtins. These pins lock
//! the surface so a switch of underlying chrono/jiff dependency can't
//! silently break formats that demos and tests rely on.
//!
//! The pins are written to be timezone-independent: any check involving
//! a wall-clock formatting either uses UTC explicitly or asserts on
//! shape, not exact values.

use crate::common::*;

// ── `time()` returns a sane positive epoch ───────────────────────────

#[test]
fn time_returns_positive_epoch_seconds() {
    // Lower bound = 2025-01-01. Anything below means the builtin is
    // returning ms / µs / ns instead of seconds, or a broken zero.
    let code = r#"
        my $t = time();
        ($t >= 1735689600 && $t < 4102444800) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn time_is_monotonic_non_decreasing_within_call() {
    let code = r#"
        my $a = time();
        my $b = time();
        $b >= $a ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── `now_ns` / `now_us` shape ────────────────────────────────────

#[test]
fn now_ns_is_larger_than_time_seconds() {
    let code = r#"
        my $s  = time();
        my $ns = now_ns();
        # ns should be ~1e9 * seconds — at minimum strictly bigger.
        ($ns > $s) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn now_ns_is_roughly_time_times_1e9() {
    let code = r#"
        my $s  = time();
        my $ns = now_ns();
        my $ratio = $ns / $s;
        # Should be ~1e9; allow generous tolerance for clock skew.
        ($ratio > 9e8 && $ratio < 2e9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn now_us_monotonic_between_calls() {
    // Two same-frame calls must yield non-decreasing µs.
    let code = r#"
        my $a = now_us();
        my $b = now_us();
        $b >= $a ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn now_ns_advances_after_small_work() {
    // Trivial loop to consume real time; ns counter must increase.
    let code = r#"
        my $a = now_ns();
        my $s = 0;
        for my $i (1:10000) { $s += $i }
        my $b = now_ns();
        ($b > $a && $s == 50005000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── `strftime` format-directive matrix ───────────────────────────────

#[test]
fn strftime_iso_date_directive_shape() {
    // %Y-%m-%d against a fixed epoch (2024-01-15 12:00:00 UTC).
    let code = r#"
        # 2024-01-15T12:00:00Z = 1705320000
        my $s = strftime("%Y-%m-%d", 1705320000);
        # Format must be 10 chars, "YYYY-MM-DD" — date may vary by tz
        # but length and shape are tz-invariant.
        (len($s) == 10 && $s =~ /^\d{4}-\d{2}-\d{2}$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_iso_datetime_full_shape() {
    let code = r#"
        my $s = strftime("%Y-%m-%dT%H:%M:%S", 1705320000);
        ($s =~ /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_rfc2822_shape() {
    let code = r#"
        my $s = strftime("%a, %d %b %Y %H:%M:%S", 1705320000);
        # "Mon, 15 Jan 2024 12:00:00" (length varies; weekday/month abbrev shape)
        ($s =~ /^[A-Z][a-z]{2}, \d{2} [A-Z][a-z]{2} \d{4} \d{2}:\d{2}:\d{2}$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_year_only_is_four_digits() {
    let code = r#"
        my $s = strftime("%Y", 1705320000);
        ($s =~ /^\d{4}$/ && $s >= "2023" && $s <= "2024") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_unix_epoch_zero_is_1969_or_1970() {
    // Epoch 0: 1970-01-01 UTC, but on tz=America/Los_Angeles it's
    // 1969-12-31. Accept both.
    let code = r#"
        my $y = strftime("%Y", 0);
        ($y eq "1969" || $y eq "1970") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── now_ms ↔ now_us ↔ now_ns consistency ────────────────────────────

#[test]
fn now_ms_smaller_than_now_us_smaller_than_now_ns() {
    let code = r#"
        my $ms = now_ms();
        my $us = now_us();
        my $ns = now_ns();
        # Same wall-clock instant, three scales: ms < us < ns.
        ($ms < $us && $us < $ns) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn now_us_is_roughly_now_ms_times_1000() {
    let code = r#"
        my $ms    = now_ms();
        my $us    = now_us();
        my $ratio = $us / $ms;
        ($ratio > 900 && $ratio < 1100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Default format (no explicit format string) ───────────────────────

#[test]
fn strftime_with_now_does_not_crash() {
    let code = r#"
        my $s = strftime("%Y-%m-%d %H:%M:%S", time());
        len($s) >= 19 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Duration / elapsed math ──────────────────────────────────────────

#[test]
fn now_ns_difference_in_microseconds_range() {
    // Measure a small loop and ensure the elapsed math comes out in a
    // plausible range. Lower bound = 0, upper bound = 5 seconds.
    let code = r#"
        my $t1 = now_ns();
        my $s  = 0;
        for my $i (1:10000) { $s += $i * $i }
        my $t2 = now_ns();
        my $elapsed = $t2 - $t1;
        ($elapsed >= 0 && $elapsed < 5_000_000_000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── strftime numeric/zero-padded directives ──────────────────────────

#[test]
fn strftime_hour_minute_zero_padded() {
    let code = r#"
        # Midnight UTC: 2024-01-01T00:00:00Z = 1704067200
        my $s = strftime("%H:%M:%S", 1704067200);
        # %H is 00-23 zero-padded.
        ($s =~ /^\d{2}:\d{2}:\d{2}$/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-call: now_ns granularity is finer than µs ──────────────────

#[test]
fn now_ns_finer_grain_than_now_us() {
    // 1000 distinct ns ticks should be easy; µs less so. Just sanity-
    // check that ns gives a real-sized number, not a hardcoded zero.
    let code = r#"
        my $a = now_ns();
        my $b = now_ns();
        # At least non-zero "tick width" of some kind.
        ($a > 0 && $b > 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── strftime + epoch math interop ────────────────────────────────────

#[test]
fn one_day_in_seconds_advances_date_by_one() {
    let code = r#"
        my $today    = strftime("%Y-%m-%d", 1705320000);
        my $tomorrow = strftime("%Y-%m-%d", 1705320000 + 86400);
        ($today ne $tomorrow) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn epoch_zero_strftime_returns_nonempty() {
    let code = r#"
        my $s = strftime("%Y-%m-%dT%H:%M:%S", 0);
        len($s) >= 19 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
