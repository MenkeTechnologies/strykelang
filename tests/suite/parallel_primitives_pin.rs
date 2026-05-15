//! Correctness pins for stryke's parallel primitives — pmap, pgrep,
//! pfor, psort, preduce, pmap_reduce, fan. These ride rayon's
//! work-stealing pool; the pins assert behavior (order, completeness,
//! determinism of associative folds) rather than perf, so they're
//! stable on CI machines with varying core counts.

use crate::common::*;

// ── pmap: order-preserving parallel map ───────────────────────────────

#[test]
fn pmap_preserves_input_order_under_load() {
    // 1000 items, each given a non-trivial body. Result indices must
    // align with input indices regardless of which core finished first.
    let code = r#"
        my @r = pmap { _ * 2 } (1:1000);
        join(",", @r[0:4]) eq "2,4,6,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pmap_returns_same_count_as_input() {
    let code = r#"
        my @r = pmap { _ + 1 } (1:500);
        len(@r) == 500 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pmap_on_empty_list_returns_empty() {
    let code = r#"
        my @empty;
        my @r = pmap { _ * 2 } @empty;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pmap_matches_serial_map_on_pure_body() {
    // For a pure function, pmap and map must produce identical lists.
    let code = r#"
        my @inputs = (1:100);
        my @serial = map  { _ * _ } @inputs;
        my @par    = pmap { _ * _ } @inputs;
        join(",", @serial) eq join(",", @par) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pgrep: parallel filter, order-preserving ─────────────────────────

#[test]
fn pgrep_filter_matches_serial_grep() {
    let code = r#"
        my @inputs = (1:200);
        my @serial = grep  { _ % 3 == 0 } @inputs;
        my @par    = pgrep { _ % 3 == 0 } @inputs;
        join(",", @serial) eq join(",", @par) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pgrep_returns_exact_match_count() {
    let code = r#"
        my @primes_ish = pgrep {
            my $n = _;
            return 0 if $n < 2;
            for my $d (2:int(sqrt($n))) { return 0 if $n % $d == 0 }
            1
        } (2:100);
        # 25 primes < 100
        len(@primes_ish) == 25 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── preduce: tree-fold over associative ops ───────────────────────────

#[test]
fn preduce_sum_matches_serial_sum() {
    // Tree fold on associative addition must match the linear sum.
    let code = r#"
        my @nums = (1:10_000);
        my $par = preduce { _0 + _1 } @nums;
        my $linear = sum(@nums);
        $par == $linear ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn preduce_max_matches_serial_max() {
    let code = r#"
        my @nums = (3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9, 7);
        my $par = preduce { _0 > _1 ? _0 : _1 } @nums;
        $par == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn preduce_string_concat_associative_only() {
    // String concat is associative (left-to-right associativity holds
    // for any partition). preduce can chunk arbitrarily but the merge
    // is in order.
    let code = r#"
        my @parts = ("a", "b", "c", "d", "e");
        my $r = preduce { _0 . _1 } @parts;
        $r eq "abcde" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn preduce_with_inline_initial_value() {
    // `preduce { BODY } INIT, @list` injects INIT as the seed for the
    // first comparison. (`preduce_init INIT, { BODY } @list` has its
    // own surface but currently rejects the init — separate bug, not
    // pinned here.) For now, pin the working form.
    let code = r#"
        my @nums = (1:100);
        my $sum_plus_1000 = preduce { _0 + _1 } 1000, @nums;
        $sum_plus_1000 == 5050 + 1000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pmap_reduce: fused map + reduce ──────────────────────────────────

#[test]
fn pmap_reduce_sum_of_squares() {
    // Σ(k=1..100) k² = 100·101·201/6 = 338350
    let code = r#"
        pmap_reduce { _ * _ } { _0 + _1 } (1:100)
    "#;
    assert_eq!(eval_int(code), 338_350);
}

#[test]
fn pmap_reduce_matches_two_step_pipeline() {
    let code = r#"
        my @nums = (1:500);
        my $fused = pmap_reduce { _ * 3 } { _0 + _1 } @nums;
        my $stepwise = sum(pmap { _ * 3 } @nums);
        $fused == $stepwise ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── psort: parallel sort matches serial sort ─────────────────────────

#[test]
fn psort_numeric_matches_serial_sort() {
    let code = r#"
        my @nums = map { int(rand() * 1_000_000) } (1:200);
        my @serial = sort  { _0 <=> _1 } @nums;
        my @par    = psort { _0 <=> _1 } @nums;
        join(",", @serial) eq join(",", @par) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn psort_string_descending() {
    let code = r#"
        my @words = ("delta", "alpha", "bravo", "charlie", "echo");
        my @desc = psort { _1 cmp _0 } @words;
        join(",", @desc) eq "echo,delta,charlie,bravo,alpha" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pfor: parallel side-effect iteration ─────────────────────────────

#[test]
fn pfor_writes_all_items_to_shared_counter() {
    // Use a KV store (which has its own internal lock) as the
    // observable side-effect — process-globals like `mysync` work too
    // but KV gives a more interesting end-to-end pin.
    let path = format!(
        "/tmp/stryke_pfor_test_{}.rkyv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            pfor {{ kv_put($db, "k$_", $_ * 2) }} (1:200);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            my $sample = kv_get($db2, "k100");
            unlink("{path}");
            ($n == 200 && $sample == 200) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── fan: N parallel invocations of a block ───────────────────────────

#[test]
fn fan_runs_block_n_times() {
    // `fan N { BLOCK }` runs the block N times with `$_` as the index.
    // Pinning the count via a KV-store side effect.
    let path = format!(
        "/tmp/stryke_fan_test_{}.rkyv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            fan 50 {{ kv_put($db, "fan_$_", 1) }};
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            unlink("{path}");
            $n == 50 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn fan_cap_returns_results_in_index_order() {
    let code = r#"
        my @r = fan_cap 10, { _ * _ };
        join(",", @r) eq "0,1,4,9,16,25,36,49,64,81" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Cross-feature: parallel sketch ingest ────────────────────────────

#[test]
fn parallel_ingest_into_bloom_preserves_no_false_negatives() {
    let code = r#"
        my $b = bloom_filter(20_000, 0.01);
        pfor { bloom_add($b, "user:$_") } (1:2000);
        my $hits = 0;
        for my $i (1:2000) {
            $hits++ if bloom_contains($b, "user:$i");
        }
        $hits == 2000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn parallel_ingest_into_hll_estimates_cardinality() {
    let code = r#"
        my $h = hll(14);
        pfor { hll_add($h, "k$_") } (1:5000);
        my $est = hll_count($h);
        my $rel = abs($est - 5000) / 5000;
        $rel < 0.02 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
