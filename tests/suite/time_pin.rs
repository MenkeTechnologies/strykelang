//! time / localtime / gmtime / mktime / strftime pins beyond
//! datetime_pin.rs.

use crate::common::*;

// ── localtime returns 9-element list ───────────────────────────────

#[test]
fn localtime_returns_nine_elements() {
    let code = r#"
        my @t = localtime(1705320000);
        len(@t) == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn localtime_year_field_is_years_since_1900() {
    let code = r#"
        # 1705320000 = 2024-01-15 12:00:00 UTC.
        my @t = localtime(1705320000);
        # @t[5] is year - 1900 = 124 for 2024.
        ($t[5] == 124) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn localtime_month_field_is_zero_indexed() {
    let code = r#"
        # 1705320000 = January (month 0).
        my @t = localtime(1705320000);
        $t[4] == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn localtime_day_of_month_correct() {
    let code = r#"
        # 1705320000 = the 15th.
        my @t = localtime(1705320000);
        $t[3] == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── gmtime ────────────────────────────────────────────────────────

#[test]
fn gmtime_returns_nine_elements() {
    let code = r#"
        my @t = gmtime(1705320000);
        len(@t) == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn gmtime_hour_independent_of_local_tz() {
    let code = r#"
        # gmtime should produce hour=12 for 12:00:00 UTC.
        my @t = gmtime(1705320000);
        $t[2] == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sanity: time() returns positive epoch ─────────────────────────

#[test]
fn time_returns_positive_epoch() {
    let code = r#"
        my $t = time();
        $t > 1700000000 ? 1 : 0   # post-Nov 2023
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Two time() calls non-decreasing ───────────────────────────────

#[test]
fn time_monotonic_non_decreasing() {
    let code = r#"
        my $a = time();
        my $b = time();
        $b >= $a ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── now_ns / now_us / now_ms ──────────────────────────────────────

#[test]
fn now_ns_us_ms_ordering() {
    let code = r#"
        my $ms = now_ms();
        my $us = now_us();
        my $ns = now_ns();
        ($ms < $us && $us < $ns) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── strftime variants ──────────────────────────────────────────────

#[test]
fn strftime_iso_date() {
    let code = r#"
        # 2024-01-15 12:00:00 UTC.
        my $s = strftime("%Y-%m-%d", 1705320000);
        $s =~ /^\d{4}-\d{2}-\d{2}$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_year_4_digits() {
    let code = r#"
        my $s = strftime("%Y", 1705320000);
        ($s eq "2024") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_zero_padded_month() {
    let code = r#"
        my $s = strftime("%m", 1705320000);
        $s eq "01" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_zero_padded_day() {
    let code = r#"
        my $s = strftime("%d", 1705320000);
        $s eq "15" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_weekday_short_name() {
    let code = r#"
        my $s = strftime("%a", 1705320000);
        # 2024-01-15 was a Monday.
        $s eq "Mon" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_month_short_name() {
    let code = r#"
        my $s = strftime("%b", 1705320000);
        $s eq "Jan" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Time difference math ──────────────────────────────────────────

#[test]
fn time_difference_in_seconds() {
    let code = r#"
        my $now = 1705320000;
        my $day_ago = $now - 86400;
        ($now - $day_ago) == 86400 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn one_year_seconds_approx() {
    let code = r#"
        # 365 * 86400 = 31_536_000.
        my $year = 365 * 86400;
        $year == 31_536_000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── strftime with multiple directives ────────────────────────────

#[test]
fn strftime_combo_format() {
    let code = r#"
        my $s = strftime("%Y-%m-%d %H:%M:%S", 1705320000);
        $s =~ /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Epoch zero handling ──────────────────────────────────────────

#[test]
fn epoch_zero_handled() {
    let code = r#"
        my @t = gmtime(0);
        # 1970-01-01 00:00:00 UTC → year = 70.
        $t[5] == 70 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Custom strftime format ───────────────────────────────────────

#[test]
fn strftime_rfc2822_shape() {
    let code = r#"
        my $s = strftime("%a, %d %b %Y", 1705320000);
        $s =~ /^[A-Z][a-z]{2}, \d{2} [A-Z][a-z]{2} \d{4}$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── strftime current time produces sensible year ────────────────

#[test]
fn strftime_current_year_at_least_2024() {
    let code = r#"
        my $y = strftime("%Y", time()) + 0;
        $y >= 2024 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── localtime in list context unpacks ─────────────────────────────

#[test]
fn localtime_destructure_works() {
    let code = r#"
        my ($sec, $min, $hour, $mday, $mon, $year, $wday, $yday, $isdst)
            = localtime(1705320000);
        # year = 124 (2024 - 1900).
        $year == 124 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Duration in human readable ────────────────────────────────────

#[test]
fn duration_breakdown_days_hours_minutes() {
    let code = r#"
        my $duration = 90061;  # 1d 1h 1m 1s
        my $days  = int($duration / 86400);
        my $hours = int(($duration % 86400) / 3600);
        my $mins  = int(($duration % 3600) / 60);
        my $secs  = $duration % 60;
        ($days == 1 && $hours == 1 && $mins == 1 && $secs == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── strftime now_ns consistency ──────────────────────────────────

#[test]
fn strftime_now_ns_pair_consistent() {
    let code = r#"
        my $t = time();
        my $year1 = strftime("%Y", $t);
        my $year2 = strftime("%Y", $t);
        $year1 eq $year2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Year leap-day calculation ────────────────────────────────────

#[test]
fn leap_year_feb_29_via_seconds() {
    let code = r#"
        # 2024-02-29 12:00:00 UTC = 1709208000.
        my @t = gmtime(1709208000);
        ($t[5] == 124 && $t[4] == 1 && $t[3] == 29) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
