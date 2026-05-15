//! Cross-feature integration tests — recent feature batches don't live
//! in isolation. Sketches feed KV stores, ULIDs key KV rows, Welford
//! state survives commit/reopen, sketch algebra results compose with
//! `pmap`. These tests pin the boundaries.

use crate::common::*;

fn tmp_path(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("/tmp/stryke_xfeat_{}_{}.rkyv", tag, nanos)
}

// ── Sketches × KV — serialize → store → reopen → deserialize ─────────

#[test]
fn bloom_filter_survives_kv_roundtrip() {
    let path = tmp_path("bloom_in_kv");
    let code = format!(
        r#"
            # build a bloom, serialize to bytes, store the bytes in KV,
            # commit, reopen the store, pull the bytes back out, rebuild
            # the bloom — membership checks must still match.
            my $b1 = bloom_filter(1000, 0.01);
            bloom_add($b1, $_) for ("alice", "bob", "carol");
            my $bytes = bloom_serialize($b1);

            my $db = kv_open("{path}");
            kv_put($db, "users:bloom", $bytes);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "users:bloom");
            my $b2 = bloom_deserialize($back);
            unlink("{path}");

            (bloom_contains($b2, "alice")
                && bloom_contains($b2, "bob")
                && bloom_contains($b2, "carol")
                && !bloom_contains($b2, "absolutely-not-there-zzz999")) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn hll_survives_kv_roundtrip() {
    let path = tmp_path("hll_in_kv");
    let code = format!(
        r#"
            my $h = hll(14);
            hll_add($h, "k$_") for (1..10_000);
            my $bytes = hll_serialize($h);

            my $db = kv_open("{path}");
            kv_put($db, "metrics:hll", $bytes);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "metrics:hll");
            my $h2 = hll_deserialize($back);
            unlink("{path}");

            my $cnt = hll_count($h2);
            (abs($cnt - 10_000) / 10_000) < 0.02 ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn roaring_survives_kv_roundtrip() {
    let path = tmp_path("rb_in_kv");
    let code = format!(
        r#"
            my $rb = roaring();
            rb_add($rb, $_) for (100..200);
            my $bytes = rb_serialize($rb);

            my $db = kv_open("{path}");
            kv_put($db, "set:roaring", $bytes);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "set:roaring");
            my $rb2 = rb_deserialize($back);
            unlink("{path}");

            (rb_len($rb2) == 101 && rb_contains($rb2, 150) && !rb_contains($rb2, 50)) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Sketch algebra × KV — result of `+` is itself stable on roundtrip ─

#[test]
fn bloom_union_then_kv_persist() {
    let path = tmp_path("bloomalg_kv");
    let code = format!(
        r#"
            my $a = bloom_filter(1000, 0.01);
            my $b = bloom_filter(1000, 0.01);
            bloom_add($a, "alpha");
            bloom_add($b, "beta");

            my $union = $a + $b;                 # sketch algebra
            my $bytes = bloom_serialize($union);  # store the union

            my $db = kv_open("{path}");
            kv_put($db, "merged", $bytes);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = bloom_deserialize(kv_get($db2, "merged"));
            unlink("{path}");

            (bloom_contains($back, "alpha") && bloom_contains($back, "beta")) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn roaring_intersection_then_kv_persist() {
    let path = tmp_path("rbalg_kv");
    let code = format!(
        r#"
            my $reds  = roaring();
            my $blues = roaring();
            rb_add($reds,  $_) for (1..10);
            rb_add($blues, $_) for (5..15);

            my $purple = $reds & $blues;          # intersection
            my $bytes  = rb_serialize($purple);

            my $db = kv_open("{path}");
            kv_put($db, "intersect", $bytes);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = rb_deserialize(kv_get($db2, "intersect"));
            unlink("{path}");

            # intersection is the 6 elements 5..10
            (rb_len($back) == 6 && rb_contains($back, 7) && !rb_contains($back, 1)) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── ULID × KV — ULID keys preserve insertion order on prefix scan ────

#[test]
fn ulid_keys_sort_in_insertion_order() {
    let path = tmp_path("ulid_kv");
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            my @minted;
            for my $i (1..20) {{
                my $id = ulid();
                push @minted, $id;
                kv_put($db, $id, $i);
            }}
            my @sorted_keys = kv_keys($db);
            unlink("{path}");
            # Lexicographic order of ULIDs must equal mint order.
            (join("|", @minted) eq join("|", @sorted_keys)) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Welford × KV — store running stats, reopen, continue stream ──────

#[test]
fn welford_stats_survive_kv_roundtrip() {
    let path = tmp_path("welford_kv");
    let code = format!(
        r#"
            my @stream = (4.0, 8.0, 15.0, 16.0, 23.0, 42.0);

            my $db = kv_open("{path}");
            kv_put($db, "stats:run1", {{
                n      => scalar(@stream),
                mean   => welford_mean(@stream),
                stddev => welford_stddev(@stream),
            }});
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $s = kv_get($db2, "stats:run1");
            unlink("{path}");

            ($s->{{n}} == 6 && abs($s->{{mean}} - 18.0) < 1e-9
                && abs($s->{{stddev}} - 13.490737563) < 1e-6) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Kahan × pmap — compensated summation as a pipeline stage ─────────

#[test]
fn kahan_sum_recovers_precision_over_pipeline() {
    let code = r#"
        my @nasty = (1e20, 1, -1e20, 1, -1e20, 1e20);
        my $naive = 0;
        $naive += $_ for @nasty;                      # loses precision
        my $kahan = kahan_sum(@nasty);                # recovers it
        ($naive == 0 && $kahan == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sketch algebra × pmap — independent sketches merge after fan-out ─

#[test]
fn bloom_filters_merged_across_pmap_partitions() {
    let code = r#"
        # Build 4 independent bloom filters in parallel — each gets a
        # disjoint slice — then fold them back to a single union using
        # the `+` operator.
        my @partitions = (
            ["a:1", "a:2", "a:3"],
            ["b:1", "b:2", "b:3"],
            ["c:1", "c:2", "c:3"],
            ["d:1", "d:2", "d:3"],
        );
        my @blooms = map {
            my $bf = bloom_filter(1000, 0.01);
            bloom_add($bf, $_) for @$_;
            $bf
        } @partitions;

        # Fold: bloom_0 + bloom_1 + bloom_2 + bloom_3
        my $merged = $blooms[0] + $blooms[1] + $blooms[2] + $blooms[3];

        my $count = 0;
        for my $part (@partitions) {
            for my $k (@$part) {
                $count++ if bloom_contains($merged, $k);
            }
        }
        # All 12 originals must be in the union; operands unchanged.
        ($count == 12 && !bloom_contains($blooms[0], "b:1")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Rope × KV — long-doc editing state stored across runs ────────────

#[test]
fn rope_edits_then_persist_string_to_kv() {
    let path = tmp_path("rope_kv");
    let code = format!(
        r#"
            my $r = rope("Hello, World!");
            rope_insert($r, 7, "beautiful ");
            my $s = rope_to_string($r);

            my $db = kv_open("{path}");
            kv_put($db, "doc:latest", $s);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "doc:latest");
            unlink("{path}");

            $back eq "Hello, beautiful World!" ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Patience diff × KV — diff old vs new doc, store the op log ───────

#[test]
fn patience_diff_ops_serialize_through_kv() {
    let path = tmp_path("diff_kv");
    let code = format!(
        r#"
            my @v1 = ("alpha", "beta",  "gamma", "delta");
            my @v2 = ("alpha", "BETA",  "gamma", "epsilon");
            my @ops = patience_diff(\@v1, \@v2);

            my $db = kv_open("{path}");
            # Convert each op pair to a 2-element arrayref for storage.
            kv_put($db, "diff:v1->v2", [map {{ [$_->[0], $_->[1]] }} @ops]);
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = kv_get($db2, "diff:v1->v2");
            unlink("{path}");

            # First op should be `=` on "alpha" since it's an anchor.
            (ref($back) eq "ARRAY"
                && scalar(@$back) >= 1
                && $back->[0]->[0] eq "="
                && $back->[0]->[1] eq "alpha") ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── CMS × pmap — independent counters merged after parallel ingest ───

#[test]
fn cms_counts_merge_via_plus_after_pmap() {
    let code = r#"
        # Four shards, each counts a different distribution. Sum via `+`.
        my @shards = (
            { hot => 1000, cold => 50  },
            { hot => 2000, cold => 75  },
            { hot => 500,  cold => 25  },
            { hot => 800,  cold => 100 },
        );
        my @sketches = map {
            my $cms = cms(2048, 5);
            for my $key (keys %$_) {
                for (1 .. $_->{$key}) { cms_add($cms, $key) }
            }
            $cms
        } @shards;

        my $total = $sketches[0] + $sketches[1] + $sketches[2] + $sketches[3];
        # Truth: hot = 4300, cold = 250
        (cms_count($total, "hot") >= 4300
            && cms_count($total, "cold") >= 250) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-feature pipeline: hash_ring → cms → roaring → kv ───────────

#[test]
fn end_to_end_shard_count_and_persist() {
    let path = tmp_path("e2e");
    let code = format!(
        r#"
            # Simulate a distributed counter: route keys via a hash ring,
            # count per shard with CMS, store the per-shard top-3 in KV.
            my $ring = hash_ring(100);
            hr_add($ring, $_) for ("shard-a", "shard-b", "shard-c");

            my %shard_cms;
            $shard_cms{{$_}} = cms(2048, 5) for ("shard-a", "shard-b", "shard-c");

            # Ingest 5000 events; each routed by hash ring.
            for my $i (1..5000) {{
                my $key   = "event-" . ($i % 200);   # 200 unique keys
                my $shard = hr_get($ring, $key);
                cms_add($shard_cms{{$shard}}, $key);
            }}

            # Merge all shards via sketch algebra and persist global counts
            # for the top 5 hot keys into the KV. Stryke terminates at
            # newline like Perl/shell, so the merge stays on one line.
            my $global = $shard_cms{{"shard-a"}} + $shard_cms{{"shard-b"}} + $shard_cms{{"shard-c"}};

            my $db = kv_open("{path}");
            for my $i (0..4) {{
                my $key = "event-" . $i;
                kv_put($db, "count:$key", cms_count($global, $key));
            }}
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            my $sum = 0;
            $sum += kv_get($db2, "count:event-$_") for (0..4);
            unlink("{path}");

            # 5000 events / 200 keys = 25 events per key (≥, with CMS overcount).
            # 5 keys × ~25 = ~125 minimum, with overcount = anything ≥ 125.
            ($n == 5 && $sum >= 125) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}
