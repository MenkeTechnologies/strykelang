//! rkyv-backed KV store builtins — Phase 1 (local). Every test uses a
//! temp file path so concurrent test execution doesn't collide.

use crate::common::*;

fn tmp_path(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("/tmp/stryke_kv_{}_{}.rkyv", tag, nanos)
}

#[test]
fn kv_open_put_get_roundtrip() {
    let path = tmp_path("rt");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "alpha", 42);
            kv_put($db, "beta", "hello");
            my $a = kv_get($db, "alpha");
            my $b = kv_get($db, "beta");
            unlink("{path}");
            ($a == 42 && $b eq "hello") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_missing_key_returns_undef() {
    let path = tmp_path("miss");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $v = kv_get($db, "ghost");
            unlink("{path}");
            defined($v) ? 0 : 1
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_del_returns_existed_flag() {
    let path = tmp_path("del");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "x", 1);
            my $first  = kv_del($db, "x");
            my $second = kv_del($db, "x");
            unlink("{path}");
            ($first == 1 && $second == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_exists_distinguishes_present_and_absent() {
    let path = tmp_path("exists");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", 0);
            my $has  = kv_exists($db, "k");
            my $miss = kv_exists($db, "no");
            unlink("{path}");
            ($has == 1 && $miss == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_len_counts_entries() {
    let path = tmp_path("len");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "a", 1);
            kv_put($db, "b", 2);
            kv_put($db, "c", 3);
            my $n = kv_len($db);
            kv_del($db, "b");
            my $m = kv_len($db);
            unlink("{path}");
            ($n == 3 && $m == 2) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_keys_returns_sorted_array() {
    let path = tmp_path("keys");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "z", 1);
            kv_put($db, "a", 1);
            kv_put($db, "m", 1);
            my @ks = kv_keys($db);
            unlink("{path}");
            ($ks[0] eq "a" && $ks[1] eq "m" && $ks[2] eq "z") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_keys_with_prefix_filters() {
    let path = tmp_path("keysprefix");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "user:1", 1);
            kv_put($db, "user:2", 1);
            kv_put($db, "log:1", 1);
            my @us = kv_keys($db, "user:");
            unlink("{path}");
            (scalar(@us) == 2 && $us[0] eq "user:1" && $us[1] eq "user:2") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_scan_returns_pairs() {
    let path = tmp_path("scan");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "user:alice", "A");
            kv_put($db, "user:bob",   "B");
            kv_put($db, "log:1",      "X");
            my @rows = kv_scan($db, "user:");
            unlink("{path}");
            # rows is array of [k,v] pairs, sorted by key
            (scalar(@rows) == 2
                && $rows[0]->[0] eq "user:alice" && $rows[0]->[1] eq "A"
                && $rows[1]->[0] eq "user:bob"   && $rows[1]->[1] eq "B") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_commit_persists_across_reopen() {
    let path = tmp_path("persist");
    let code = format!(
        r#"
            my $db1 = kv_open("{path}");
            kv_put($db1, "k1", 100);
            kv_put($db1, "k2", "hi");
            kv_commit($db1);
            my $db2 = kv_open("{path}");
            my $a = kv_get($db2, "k1");
            my $b = kv_get($db2, "k2");
            my $n = kv_len($db2);
            unlink("{path}");
            ($a == 100 && $b eq "hi" && $n == 2) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_close_auto_commits() {
    let path = tmp_path("close");
    let code = format!(
        r#"
            my $db1 = kv_open("{path}");
            kv_put($db1, "auto", 7);
            kv_close($db1);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "auto");
            unlink("{path}");
            $v == 7 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_batch_applies_all_ops() {
    let path = tmp_path("batch");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "seed", 1);
            my $n = kv_batch($db, [
                ["put", "a", 10],
                ["put", "b", 20],
                ["del", "seed"],
                ["put", "c", 30],
            ]);
            my $exists_seed = kv_exists($db, "seed");
            my $sum = kv_get($db, "a") + kv_get($db, "b") + kv_get($db, "c");
            unlink("{path}");
            ($n == 4 && $exists_seed == 0 && $sum == 60) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_batch_rolls_back_on_bad_op() {
    let path = tmp_path("batchroll");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "before", 1);
            my $err = 0;
            eval {{
                kv_batch($db, [
                    ["put", "x", 1],
                    ["wat", "y", 2],   # unknown op kind
                ]);
            }};
            $err = 1 if $@;
            # After rollback, "before" still present and "x" must not be.
            my $b = kv_exists($db, "before");
            my $x = kv_exists($db, "x");
            unlink("{path}");
            ($err == 1 && $b == 1 && $x == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_nested_array_roundtrip() {
    let path = tmp_path("nested");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "deep", [1, [2, 3], [[4, 5], 6]]);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "deep");
            unlink("{path}");
            # v->[0] = 1, v->[1]->[1] = 3, v->[2]->[0]->[1] = 5
            ($v->[0] == 1 && $v->[1]->[1] == 3 && $v->[2]->[0]->[1] == 5) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_nested_hash_roundtrip() {
    let path = tmp_path("nestedh");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "user", {{ name => "alice", age => 30, addr => {{ city => "Atlanta" }} }});
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $u = kv_get($db2, "user");
            unlink("{path}");
            ($u->{{name}} eq "alice" && $u->{{age}} == 30 && $u->{{addr}}->{{city}} eq "Atlanta") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_stats_returns_expected_shape() {
    let path = tmp_path("stats");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "x", 1);
            my $s = kv_stats($db);
            unlink("{path}");
            # Returns a hash with entries / format_version / commit_count fields.
            ($s->{{entries}} == 1 && $s->{{format_version}} == 1) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_ref_type_is_kvstore() {
    let path = tmp_path("reftype");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $t = ref($db);
            unlink("{path}");
            $t =~ /KvStore/ ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_builtins_visible_in_b_hash() {
    for name in &[
        "kv_open",
        "kv_put",
        "kv_get",
        "kv_del",
        "kv_exists",
        "kv_keys",
        "kv_scan",
        "kv_len",
        "kv_commit",
        "kv_batch",
        "kv_close",
        "kv_stats",
    ] {
        let code = format!(r#"exists $b{{{name}}} ? 1 : 0"#);
        assert_eq!(eval_int(&code), 1, "missing from %b: {name}");
    }
}
