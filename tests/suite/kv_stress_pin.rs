//! rkyv KV store edge-case + stress pins. Phase 1 (local) has been
//! shipped for a while; these pins cover the durability + correctness
//! corners that aren't in `tests/suite/kvstore.rs` (smoke tests).

use crate::common::*;

fn tmp_path(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("/tmp/stryke_kv_stress_{}_{}.rkyv", tag, nanos)
}

// ── Parallel pfor → kv_put: KvStore's internal Mutex serialises ──────

#[test]
fn pfor_kv_put_writes_all_items_correctly() {
    let path = tmp_path("pfor");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            pfor {{ kv_put($db, "k$_", $_ * 10) }} (1:500);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            # Spot-check a few entries across the range.
            my $ok =  kv_get($db2, "k1")    ==   10
                   && kv_get($db2, "k250")  == 2500
                   && kv_get($db2, "k500")  == 5000;
            unlink("{path}");
            ($n == 500 && $ok) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Large value persistence ──────────────────────────────────────────

#[test]
fn large_string_value_round_trips() {
    // 100 KB string — well below the rkyv 4 KB scratch buffer hint but
    // exercises serializer growth.
    let path = tmp_path("large");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $big = "x" x 100_000;
            kv_put($db, "big", $big);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "big");
            unlink("{path}");
            (len($back) == 100_000 && $back eq $big) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn long_array_value_round_trips() {
    let path = tmp_path("longarr");
    let code = format!(
        r#"
            my @nums = (1:1000);
            my $db = kv_open("{path}");
            kv_put($db, "nums", \@nums);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "nums");
            unlink("{path}");
            (len($back) == 1000 && $back->[0] == 1 && $back->[999] == 1000) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Unicode keys and values ──────────────────────────────────────────

#[test]
fn unicode_keys_round_trip() {
    let path = tmp_path("unicode_keys");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "ключ",   "значение");
            kv_put($db, "鍵",      "値");
            kv_put($db, "🔑",     "🔐 vault");
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $ok =  kv_get($db2, "ключ") eq "значение"
                   && kv_get($db2, "鍵")    eq "値"
                   && kv_get($db2, "🔑")   eq "🔐 vault";
            unlink("{path}");
            $ok ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn empty_string_key_and_value_round_trip() {
    let path = tmp_path("empty");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "", "empty-key-value");
            kv_put($db, "non-empty", "");
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $ok =  kv_get($db2, "")          eq "empty-key-value"
                   && kv_get($db2, "non-empty") eq "";
            unlink("{path}");
            $ok ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Key iteration order: sorted lexicographically ────────────────────

#[test]
fn keys_are_sorted_lexicographically() {
    let path = tmp_path("sortkeys");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            # Intentionally insert out of order.
            kv_put($db, "zeta",  1);
            kv_put($db, "alpha", 2);
            kv_put($db, "mu",    3);
            kv_put($db, "beta",  4);
            my @keys = kv_keys($db);
            unlink("{path}");
            join(",", @keys) eq "alpha,beta,mu,zeta" ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn keys_with_prefix_filter_match_sorted() {
    let path = tmp_path("prefix");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "user:bob",   1);
            kv_put($db, "user:alice", 1);
            kv_put($db, "log:1",      1);
            kv_put($db, "user:carol", 1);
            my @users = kv_keys($db, "user:");
            unlink("{path}");
            (len(@users) == 3
                && $users[0] eq "user:alice"
                && $users[1] eq "user:bob"
                && $users[2] eq "user:carol") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── kv_batch atomicity ───────────────────────────────────────────────

#[test]
fn batch_applies_all_ops_when_all_valid() {
    let path = tmp_path("batch_ok");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $n = kv_batch($db, [
                ["put", "a", 1],
                ["put", "b", 2],
                ["put", "c", 3],
            ]);
            my @ks = kv_keys($db);
            unlink("{path}");
            ($n == 3 && join(",", @ks) eq "a,b,c") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn batch_rolls_back_on_unknown_op_kind() {
    let path = tmp_path("batch_bad");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "before", 1);
            my $err = 0;
            eval {{
                kv_batch($db, [
                    ["put", "x", 1],
                    ["put", "y", 2],
                    ["NUKE", "*"],
                ]);
            }};
            $err = 1 if $@;
            my $still_before = kv_exists($db, "before");
            my $no_x         = !kv_exists($db, "x");
            my $no_y         = !kv_exists($db, "y");
            unlink("{path}");
            ($err == 1 && $still_before && $no_x && $no_y) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── kv_stats tracks dirty/clean + commit_count ───────────────────────

#[test]
fn stats_commit_count_increments_only_on_dirty_commit() {
    let path = tmp_path("commits");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "a", 1);
            kv_commit($db);   # commit #1
            kv_commit($db);   # no-op (clean)
            kv_put($db, "b", 2);
            kv_commit($db);   # commit #2
            my $cc = kv_stats($db)->{{commit_count}};
            unlink("{path}");
            $cc == 2 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn stats_dirty_flag_flips_with_writes() {
    let path = tmp_path("dirty");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $clean1 = kv_stats($db)->{{dirty}};
            kv_put($db, "k", 1);
            my $dirty1 = kv_stats($db)->{{dirty}};
            kv_commit($db);
            my $clean2 = kv_stats($db)->{{dirty}};
            unlink("{path}");
            ($clean1 == 0 && $dirty1 == 1 && $clean2 == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── kv_close auto-commits if dirty ───────────────────────────────────

#[test]
fn close_auto_commits_dirty_state() {
    let path = tmp_path("close");
    let code = format!(
        r#"
            {{
                my $db = kv_open("{path}");
                kv_put($db, "auto", 99);
                kv_close($db);
            }}
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "auto");
            unlink("{path}");
            $v == 99 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Nested values: arrayref + hashref deep ───────────────────────────

#[test]
fn deeply_nested_value_round_trips() {
    let path = tmp_path("deep");
    let code = format!(
        r#"
            my $deep = +{{
                name  => "alice",
                age   => 30,
                tags  => ["admin", "trusted", "remote"],
                meta  => +{{
                    last_seen  => "2026-05-15",
                    locations  => +{{
                        home => "atlanta",
                        work => "remote",
                    }},
                }},
            }};
            my $db = kv_open("{path}");
            kv_put($db, "user", $deep);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "user");
            unlink("{path}");
            ($back->{{name}} eq "alice"
                && $back->{{tags}}->[1] eq "trusted"
                && $back->{{meta}}->{{locations}}->{{home}} eq "atlanta") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Re-opening missing file returns empty store ──────────────────────

#[test]
fn open_missing_path_creates_empty_store() {
    let path = tmp_path("missing");
    let code = format!(
        r#"
            # Ensure file does NOT exist.
            unlink("{path}");
            my $db = kv_open("{path}");
            my $n  = kv_len($db);
            unlink("{path}");
            $n == 0 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}
