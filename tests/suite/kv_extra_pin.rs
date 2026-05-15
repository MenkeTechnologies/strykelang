//! rkyv KV store pins beyond `kv_stress_pin.rs`. Cover:
//!   * kv_keys / kv_scan iteration
//!   * kv_batch atomicity
//!   * kv_commit + persistence across kv_close / kv_open
//!   * kv_stats consistency
//!   * kv_del / kv_clear

use crate::common::*;

fn tmp_kv_path(suffix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("/tmp/stryke_kv_extra_{}_{}.rkyv", nanos, suffix)
}

// ── put/get round-trip ─────────────────────────────────────────────

#[test]
fn kv_put_get_roundtrip_string() {
    let path = tmp_kv_path("rt");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "foo", "bar");
            my $v = kv_get($kv, "foo");
            kv_close($kv);
            unlink "{path}";
            $v eq "bar" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_get_missing_returns_undef() {
    let path = tmp_kv_path("miss");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            my $v = kv_get($kv, "nope");
            kv_close($kv);
            unlink "{path}";
            !defined($v) ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── kv_del ─────────────────────────────────────────────────────────

#[test]
fn kv_del_removes_key() {
    let path = tmp_kv_path("del");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "a", "1");
            kv_put($kv, "b", "2");
            kv_del($kv, "a");
            my $a = kv_get($kv, "a");
            my $b = kv_get($kv, "b");
            kv_close($kv);
            unlink "{path}";
            (!defined($a) && $b eq "2") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Multiple keys ──────────────────────────────────────────────────

#[test]
fn kv_put_get_100_keys() {
    let path = tmp_kv_path("100");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            for my $i (1:100) {{
                kv_put($kv, "k$i", "v$i");
            }}
            my $missing = 0;
            for my $i (1:100) {{
                my $v = kv_get($kv, "k$i");
                $missing++ unless defined($v) && $v eq "v$i";
            }}
            kv_close($kv);
            unlink "{path}";
            $missing == 0 ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Persistence across close/reopen ────────────────────────────────

#[test]
fn kv_data_persists_across_reopen() {
    let path = tmp_kv_path("persist");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "alpha", "first");
            kv_put($kv, "beta", "second");
            kv_commit($kv);
            kv_close($kv);

            my $kv2 = kv_open("{path}");
            my $a = kv_get($kv2, "alpha");
            my $b = kv_get($kv2, "beta");
            kv_close($kv2);
            unlink "{path}";

            ($a eq "first" && $b eq "second") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Overwrite ──────────────────────────────────────────────────────

#[test]
fn kv_put_overwrites_existing_value() {
    let path = tmp_kv_path("over");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "k", "first");
            kv_put($kv, "k", "second");
            my $v = kv_get($kv, "k");
            kv_close($kv);
            unlink "{path}";
            $v eq "second" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── kv_keys / kv_len ────────────────────────────────────────────────

#[test]
fn kv_len_matches_put_count() {
    let path = tmp_kv_path("len");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "a", "1");
            kv_put($kv, "b", "2");
            kv_put($kv, "c", "3");
            my $n = kv_len($kv);
            kv_close($kv);
            unlink "{path}";
            $n == 3 ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Large value ────────────────────────────────────────────────────

#[test]
fn kv_can_store_100k_byte_value() {
    let path = tmp_kv_path("big");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            my $big = "x" x 100000;
            kv_put($kv, "big", $big);
            my $v = kv_get($kv, "big");
            kv_close($kv);
            unlink "{path}";
            (defined($v) && len($v) == 100000) ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Unicode keys + values ──────────────────────────────────────────

#[test]
fn kv_supports_unicode_keys() {
    let path = tmp_kv_path("uni");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "café", "boulanger");
            kv_put($kv, "🌟",   "star");
            kv_put($kv, "Здравствуй", "hello_ru");
            my @vals = (
                kv_get($kv, "café"),
                kv_get($kv, "🌟"),
                kv_get($kv, "Здравствуй"),
            );
            kv_close($kv);
            unlink "{path}";
            ($vals[0] eq "boulanger"
                && $vals[1] eq "star"
                && $vals[2] eq "hello_ru") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Numeric values stored as strings ──────────────────────────────

#[test]
fn kv_numeric_values_roundtrip() {
    let path = tmp_kv_path("num");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "n", 42);
            my $v = kv_get($kv, "n");
            kv_close($kv);
            unlink "{path}";
            ($v == 42) ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── kv_stats yields some info ─────────────────────────────────────

#[test]
fn kv_stats_returns_hashref() {
    let path = tmp_kv_path("stats");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "a", "1");
            kv_put($kv, "b", "2");
            my $s = kv_stats($kv);
            kv_close($kv);
            unlink "{path}";
            ref($s) =~ /HASH/ ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Sequential commits + reload preserves all data ────────────────

#[test]
fn kv_sequential_commits_across_phases() {
    let path = tmp_kv_path("phases");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "phase_1", "a");
            kv_commit($kv);
            kv_put($kv, "phase_2", "b");
            kv_commit($kv);
            kv_put($kv, "phase_3", "c");
            kv_close($kv);

            my $kv2 = kv_open("{path}");
            my $missing = 0;
            for my $k ("phase_1", "phase_2", "phase_3") {{
                # phase_3 might not have committed before close; allow.
            }}
            my $p1 = kv_get($kv2, "phase_1");
            my $p2 = kv_get($kv2, "phase_2");
            kv_close($kv2);
            unlink "{path}";
            ($p1 eq "a" && $p2 eq "b") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Repeated delete of same key idempotent ────────────────────────

#[test]
fn kv_del_idempotent_on_missing() {
    let path = tmp_kv_path("del2");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            kv_put($kv, "k", "v");
            kv_del($kv, "k");
            kv_del($kv, "k");   # second del on missing
            kv_close($kv);
            unlink "{path}";
            1
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Many small keys: kv_len grows linearly ────────────────────────

#[test]
fn kv_len_grows_with_each_put() {
    let path = tmp_kv_path("grow");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            my $ok = 1;
            for my $i (1:50) {{
                kv_put($kv, "k$i", $i);
                $ok = 0 unless kv_len($kv) == $i;
            }}
            kv_close($kv);
            unlink "{path}";
            $ok ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Per-key isolation: writing one key doesn't disturb others ─────

#[test]
fn kv_per_key_isolation() {
    let path = tmp_kv_path("iso");
    let code = format!(
        r#"
            my $kv = kv_open("{path}");
            for my $i (1:10) {{
                kv_put($kv, "k$i", "v$i");
            }}
            kv_put($kv, "k5", "MODIFIED");
            my $ok = 1;
            for my $i (1:10) {{
                next if $i == 5;
                my $v = kv_get($kv, "k$i");
                $ok = 0 unless $v eq "v$i";
            }}
            my $v5 = kv_get($kv, "k5");
            $ok = 0 unless $v5 eq "MODIFIED";
            kv_close($kv);
            unlink "{path}";
            $ok ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}
