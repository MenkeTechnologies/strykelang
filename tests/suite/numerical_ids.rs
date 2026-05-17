//! Tier-1 zero-bloat additions: ULID, Kahan summation, Welford
//! online stats. Pure-function builtins (no new crates, no HeapObject
//! variants). Pinned at the stryke-script level so the surface stays
//! stable.

use crate::common::*;

// ── ULID ──────────────────────────────────────────────────────────────

#[test]
fn ulid_is_26_chars() {
    assert_eq!(eval_int(r#"length(ulid())"#), 26);
}

#[test]
fn ulid_is_crockford_base32() {
    // Every char must be 0-9 or A-Z minus I L O U (Crockford).
    let code = r#"
        my $u = ulid();
        my $ok = 1;
        for my $c (split //, $u) {
            $ok = 0 unless $c =~ /^[0-9A-HJKMNP-TV-Z]$/;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ulid_is_lexicographically_sortable() {
    // ULIDs from sequential calls must sort in chronological order.
    let code = r#"
        my @ids = (ulid(), ulid(), ulid(), ulid(), ulid());
        my @sorted = sort @ids;
        join(",", @ids) eq join(",", @sorted) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn is_ulid_validates_format() {
    assert_eq!(eval_int(r#"is_ulid(ulid())"#), 1);
    assert_eq!(eval_int(r#"is_ulid("not-a-ulid")"#), 0);
    assert_eq!(eval_int(r#"is_ulid("01KRJ8YC8AGW5XHM2BKHJ23ZRR")"#), 1);
    // Wrong length:
    assert_eq!(eval_int(r#"is_ulid("01KRJ8YC8A")"#), 0);
    // Contains 'I' (forbidden in Crockford):
    assert_eq!(eval_int(r#"is_ulid("01KRJ8IC8AGW5XHM2BKHJ23ZRR")"#), 0);
}

#[test]
fn ulid_timestamp_round_trips_via_time() {
    // The 48-bit timestamp in a fresh ULID should be within 1 second
    // of `time() * 1000` (allow CI clock jitter).
    let code = r#"
        my $now_ms = int(time() * 1000);
        my $u = ulid();
        my $ts = ulid_timestamp($u);
        abs($ts - $now_ms) < 2000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ulid_timestamp_rejects_invalid() {
    assert_eq!(
        eval_int(r#"defined(ulid_timestamp("not a ulid")) ? 1 : 0"#),
        0
    );
}

#[test]
fn ulid_alias_ulid_new() {
    assert_eq!(eval_int(r#"length(ulid_new())"#), 26);
}

// ── Kahan summation ───────────────────────────────────────────────────

#[test]
fn kahan_recovers_precision_naive_sum_loses() {
    // 1e20 + 1 + -1e20 + 1 + -1e20 + 1e20 = 2 exactly.
    // Naive f64 sum collapses to 0 because (1e20 + 1) == 1e20 at f64
    // precision. Kahan compensates.
    let naive = eval_string(r#"sum(1e20, 1, -1e20, 1, -1e20, 1e20)"#);
    let kahan = eval_string(r#"kahan_sum(1e20, 1, -1e20, 1, -1e20, 1e20)"#);
    assert_eq!(naive, "0", "expected naive to lose precision");
    assert_eq!(kahan, "2", "expected Kahan to recover");
}

#[test]
fn kahan_empty_is_zero() {
    assert_eq!(eval_string(r#"kahan_sum()"#), "0");
}

#[test]
fn kahan_single_element() {
    assert_eq!(eval_string(r#"kahan_sum(3.14)"#), "3.14");
}

#[test]
fn kahan_matches_naive_on_clean_inputs() {
    // When precision isn't lost, results should agree to >= 15 digits.
    let code = r#"
        my @xs = (1.1, 2.2, 3.3, 4.4, 5.5);
        my $k = kahan_sum(@xs);
        my $n = sum(@xs);
        abs($k - $n) < 1e-12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn kahan_aliases_route_to_same_impl() {
    // kahan / neumaier_sum / kahan_sum all route to the same handler.
    let a = eval_string(r#"kahan(1e20, 1, -1e20)"#);
    let b = eval_string(r#"neumaier_sum(1e20, 1, -1e20)"#);
    let c = eval_string(r#"kahan_sum(1e20, 1, -1e20)"#);
    assert_eq!(a, "1");
    assert_eq!(a, b);
    assert_eq!(b, c);
}

// ── Welford online stats ──────────────────────────────────────────────

#[test]
fn welford_mean_matches_textbook() {
    // (2+4+4+4+5+5+7+9)/8 = 5
    assert_eq!(eval_string(r#"welford_mean(2, 4, 4, 4, 5, 5, 7, 9)"#), "5");
}

#[test]
fn welford_variance_uses_sample_denominator() {
    // Variance of (2,4,4,4,5,5,7,9) with mean=5, sum-sq-dev=32:
    //   sample (n-1): 32/7 = 4.5714...
    //   population (n): 32/8 = 4.0
    let code = r#"
        my @d = (2, 4, 4, 4, 5, 5, 7, 9);
        my $sample = welford_variance(@d);
        my $pop = welford_pop_variance(@d);
        abs($sample - 32.0/7.0) < 1e-9 && abs($pop - 4.0) < 1e-9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn welford_stddev_is_sqrt_of_variance() {
    let code = r#"
        my @d = (2, 4, 4, 4, 5, 5, 7, 9);
        my $v = welford_variance(@d);
        my $s = welford_stddev(@d);
        abs($s - sqrt($v)) < 1e-12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn welford_empty_is_zero() {
    assert_eq!(eval_string(r#"welford_mean()"#), "0");
    assert_eq!(eval_string(r#"welford_variance()"#), "0");
    assert_eq!(eval_string(r#"welford_stddev()"#), "0");
}

#[test]
fn welford_single_element_variance_is_zero() {
    // Sample variance is undefined for n<2; we return 0 (matches stryke
    // `variance` convention).
    assert_eq!(eval_string(r#"welford_variance(42)"#), "0");
}

#[test]
fn welford_stable_on_long_constant_stream() {
    // 100k copies of 1.0; mean must be exactly 1.0, variance exactly 0.
    let code = r#"
        my @data = (1.0) x 100_000;
        my $m = welford_mean(@data);
        my $v = welford_variance(@data);
        abs($m - 1.0) < 1e-9 && $v == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reflection / aliases ──────────────────────────────────────────────

#[test]
fn new_builtins_appear_in_b_hash() {
    for name in &[
        "ulid",
        "is_ulid",
        "ulid_timestamp",
        "kahan_sum",
        "welford_mean",
        "welford_variance",
        "welford_stddev",
        "welford_pop_variance",
    ] {
        let code = format!(r#"exists $b{{{name}}} ? 1 : 0"#);
        assert_eq!(eval_int(&code), 1, "{name} missing from %b");
    }
}
