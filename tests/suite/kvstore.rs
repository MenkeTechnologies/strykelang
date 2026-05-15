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

// ── Type-by-type WireValue roundtrip ─────────────────────────────────

#[test]
fn kv_undef_roundtrip() {
    let path = tmp_path("undef");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", undef);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "k");
            unlink("{path}");
            # Key exists but value is undef. exists should be true, defined false.
            (kv_exists($db2, "k") == 1 && !defined($v)) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_negative_integer_roundtrip() {
    let path = tmp_path("negint");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "a", -42);
            kv_put($db, "b", -9223372036854775807);   # near-i64::MIN
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $a = kv_get($db2, "a");
            my $b = kv_get($db2, "b");
            unlink("{path}");
            ($a == -42 && $b == -9223372036854775807) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_large_integer_roundtrip() {
    let path = tmp_path("bigint");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "max", 9223372036854775807);   # i64::MAX
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "max");
            unlink("{path}");
            $v == 9223372036854775807 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_float_bit_identical_roundtrip() {
    let path = tmp_path("float");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "pi",  3.141592653589793);
            kv_put($db, "neg", -2.718281828459045);
            kv_put($db, "tiny", 1e-300);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $pi   = kv_get($db2, "pi");
            my $neg  = kv_get($db2, "neg");
            my $tiny = kv_get($db2, "tiny");
            unlink("{path}");
            (abs($pi - 3.141592653589793) < 1e-15
                && abs($neg + 2.718281828459045) < 1e-15
                && abs($tiny - 1e-300) < 1e-310) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_empty_string_roundtrip() {
    let path = tmp_path("emptystr");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", "");
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "k");
            unlink("{path}");
            (defined($v) && $v eq "" && length($v) == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_unicode_keys_and_values_roundtrip() {
    let path = tmp_path("utf8");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "ключ",  "значение");
            kv_put($db, "鍵",     "値");
            kv_put($db, "🔑",    "🔐 vault");
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $a = kv_get($db2, "ключ");
            my $b = kv_get($db2, "鍵");
            my $c = kv_get($db2, "🔑");
            unlink("{path}");
            ($a eq "значение" && $b eq "値" && $c eq "🔐 vault") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_string_with_special_chars_roundtrip() {
    let path = tmp_path("special");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "newlines", "line1\nline2\nline3");
            kv_put($db, "tab",      "col1\tcol2");
            kv_put($db, "quotes",   q{{he said "hi"}});
            kv_put($db, "backslash", q{{a\b\c}});
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $n = kv_get($db2, "newlines");
            my $t = kv_get($db2, "tab");
            my $q = kv_get($db2, "quotes");
            my $b = kv_get($db2, "backslash");
            unlink("{path}");
            ($n eq "line1\nline2\nline3"
                && $t eq "col1\tcol2"
                && $q eq q{{he said "hi"}}
                && $b eq q{{a\b\c}}) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_empty_array_roundtrip() {
    let path = tmp_path("emptyarr");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", []);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "k");
            unlink("{path}");
            (ref($v) eq "ARRAY" && scalar(@$v) == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_empty_hash_roundtrip() {
    let path = tmp_path("emptyhash");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", {{}});
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "k");
            unlink("{path}");
            (ref($v) eq "HASH" && scalar(keys %$v) == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Operational semantics ─────────────────────────────────────────────

#[test]
fn kv_put_returns_old_value_on_overwrite() {
    let path = tmp_path("overwrite");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $first_old = kv_put($db, "k", 1);   # no prior value
            my $second_old = kv_put($db, "k", 2);  # prior was 1
            my $third_old = kv_put($db, "k", 3);   # prior was 2
            my $final = kv_get($db, "k");
            unlink("{path}");
            (!defined($first_old) && $second_old == 1 && $third_old == 2 && $final == 3) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_commit_idempotent_when_clean() {
    // Two commits in a row: first writes, second is a no-op (dirty=false).
    let path = tmp_path("idem");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", 1);
            my $a = kv_commit($db);
            my $b = kv_commit($db);   # already clean — no-op
            my $stats = kv_stats($db);
            unlink("{path}");
            # commit_count went up exactly once (first commit), second was clean.
            ($a == 1 && $b == 1 && $stats->{{commit_count}} == 1) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_commit_count_increments_per_dirty_commit() {
    let path = tmp_path("ccount");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "a", 1);
            kv_commit($db);              # commit #1
            kv_put($db, "b", 2);
            kv_commit($db);              # commit #2
            kv_put($db, "c", 3);
            kv_commit($db);              # commit #3
            my $stats = kv_stats($db);
            unlink("{path}");
            $stats->{{commit_count}} == 3 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_stats_dirty_flag_tracks_state() {
    let path = tmp_path("dirty");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $s0 = kv_stats($db);
            kv_put($db, "k", 1);
            my $s1 = kv_stats($db);
            kv_commit($db);
            my $s2 = kv_stats($db);
            unlink("{path}");
            ($s0->{{dirty}} == 0 && $s1->{{dirty}} == 1 && $s2->{{dirty}} == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_scan_empty_prefix_returns_all() {
    let path = tmp_path("scanall");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "a", 1);
            kv_put($db, "b", 2);
            kv_put($db, "c", 3);
            my @rows = kv_scan($db, "");
            unlink("{path}");
            scalar(@rows) == 3 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_scan_no_matches_returns_empty() {
    let path = tmp_path("scannone");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "alpha", 1);
            my @rows = kv_scan($db, "zzz:");
            unlink("{path}");
            scalar(@rows) == 0 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_keys_on_empty_store_is_empty() {
    let path = tmp_path("emptykeys");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my @ks = kv_keys($db);
            my $n = kv_len($db);
            unlink("{path}");
            (scalar(@ks) == 0 && $n == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_batch_with_empty_ops_returns_zero() {
    let path = tmp_path("emptybatch");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my $n = kv_batch($db, []);
            unlink("{path}");
            $n == 0 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_reopen_after_multiple_commits() {
    // Simulates real workload: open, write, commit, write, commit, reopen.
    // Verifies the atomic-rewrite never leaves a half-written archive.
    let path = tmp_path("multicommit");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "round1", "a");
            kv_commit($db);
            kv_put($db, "round2", "b");
            kv_commit($db);
            kv_put($db, "round3", "c");
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            my $r1 = kv_get($db2, "round1");
            my $r2 = kv_get($db2, "round2");
            my $r3 = kv_get($db2, "round3");
            unlink("{path}");
            ($n == 3 && $r1 eq "a" && $r2 eq "b" && $r3 eq "c") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_many_entries_scale() {
    // Insert 5000 entries, verify all readable, prefix scan returns a subset,
    // and keys come back sorted.
    let path = tmp_path("scale");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            for my $i (1..5000) {{ kv_put($db, sprintf("k%05d", $i), $i) }}
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            my @ks = kv_keys($db2);
            my $first_ok = $ks[0] eq "k00001";
            my $last_ok  = $ks[$#ks] eq "k05000";

            # Prefix scan: keys k00001..k00099 → 99 entries (k00001..k00099).
            # Use a prefix that catches exactly the 9 entries k00001..k00009.
            my @sub = kv_keys($db2, "k0000");
            unlink("{path}");
            ($n == 5000 && $first_ok && $last_ok && scalar(@sub) == 9) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_open_corrupt_file_rejects() {
    // Write garbage bytes to a path, then attempt to open it — should die
    // (not crash) with a stryke runtime error.
    use std::io::Write;
    let path = tmp_path("corrupt");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(b"this is not a stryke kv archive").unwrap();
    drop(f);
    let code = format!(
        r#"
            my $err = 0;
            eval {{ my $db = kv_open("{path}"); }};
            $err = 1 if $@;
            unlink("{path}");
            $err == 1 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Alias coverage — every alias dispatches to the same handler ───────

#[test]
fn kv_put_set_aliases_are_equivalent() {
    let path = tmp_path("aliasput");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_set($db, "a", 1);
            kv_put($db, "b", 2);
            my $n = kv_len($db);
            unlink("{path}");
            ($n == 2 && kv_get($db, "a") == 1 && kv_get($db, "b") == 2) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_del_remove_delete_aliases() {
    let path = tmp_path("aliasdel");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "a", 1);
            kv_put($db, "b", 2);
            kv_put($db, "c", 3);
            kv_remove($db, "a");
            kv_delete($db, "b");
            kv_del($db, "c");
            my $n = kv_len($db);
            unlink("{path}");
            $n == 0 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_has_exists_aliases_match() {
    let path = tmp_path("aliashas");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "x", 1);
            my $a = kv_exists($db, "x");
            my $b = kv_has($db, "x");
            my $c = kv_exists($db, "missing");
            my $d = kv_has($db, "missing");
            unlink("{path}");
            ($a == 1 && $b == 1 && $c == 0 && $d == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_size_count_len_aliases_match() {
    let path = tmp_path("aliaslen");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, $_, 1) for ("a", "b", "c", "d");
            my $a = kv_len($db);
            my $b = kv_count($db);
            my $c = kv_size($db);
            unlink("{path}");
            ($a == 4 && $b == 4 && $c == 4) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_flush_alias_for_commit() {
    let path = tmp_path("aliasflush");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", 99);
            kv_flush($db);          # alias for kv_commit
            my $db2 = kv_open("{path}");
            my $v = kv_get($db2, "k");
            unlink("{path}");
            $v == 99 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_info_alias_for_stats() {
    let path = tmp_path("aliasinfo");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "k", 1);
            my $s = kv_info($db);
            unlink("{path}");
            $s->{{entries}} == 1 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn kv_new_alias_for_open() {
    let path = tmp_path("aliasnew");
    let code = format!(
        r#"
            my $db = kv_new("{path}");
            kv_put($db, "k", 1);
            unlink("{path}");
            kv_get($db, "k") == 1 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Errors / type mismatches ──────────────────────────────────────────

#[test]
fn kv_get_on_non_store_dies() {
    // Passing a plain integer where a KvStore handle is expected should die.
    let code = r#"
        my $err = 0;
        eval {
            kv_get(42, "k");
        };
        $err = 1 if $@;
        $err == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn kv_batch_unknown_op_kind_dies_and_rolls_back() {
    let path = tmp_path("batchbad");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            kv_put($db, "seed", 7);
            my $err = 0;
            eval {{
                kv_batch($db, [["put", "x", 1], ["nuke", "y"], ["put", "z", 2]]);
            }};
            $err = 1 if $@;
            # After roll back: seed kept, x and z absent.
            my $has_seed = kv_exists($db, "seed");
            my $has_x    = kv_exists($db, "x");
            my $has_z    = kv_exists($db, "z");
            unlink("{path}");
            ($err == 1 && $has_seed == 1 && $has_x == 0 && $has_z == 0) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}
