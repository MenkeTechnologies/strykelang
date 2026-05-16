//! Hash iteration-order pins. Stryke hash iteration order is NOT
//! guaranteed across runs; tests must sort or use a reference set.

use crate::common::*;

// ── keys returns all keys ──────────────────────────────────────────

#[test]
fn keys_returns_complete_set() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        my @ks = sort { _0 cmp _1 } keys %h;
        join(",", @ks) eq "a,b,c,d" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys count matches hash size ──────────────────────────────────

#[test]
fn keys_count_matches_assignment() {
    let code = r#"
        my %h;
        for my $i (1:100) {
            $h{"k$i"} = $i;
        }
        len(keys %h) == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── values count matches keys count ───────────────────────────────

#[test]
fn values_count_matches_keys_count() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        len(values %h) == len(keys %h) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pair (key, value) per iteration consistent ────────────────────

#[test]
fn each_kv_pair_matches() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        my $ok = 1;
        for my $k (keys %h) {
            $ok = 0 unless $h{$k} > 0;
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sort + grep preserves sort order ──────────────────────────────

#[test]
fn sort_then_grep_preserves_order() {
    let code = r#"
        my %h = (z => 1, a => 2, m => 3, b => 4);
        my @sorted = sort { _0 cmp _1 } keys %h;
        my @filtered = grep { $_ ne "m" } @sorted;
        join(",", @filtered) eq "a,b,z" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iteration over empty hash visits nothing ──────────────────────

#[test]
fn empty_hash_iteration_visits_zero() {
    let code = r#"
        my %empty;
        my $count = 0;
        for my $k (keys %empty) {
            $count++;
        }
        $count == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Same hash, two iterations, same set ───────────────────────────

#[test]
fn same_hash_two_iterations_same_keyset() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my %first;
        my %second;
        $first{$_}++ for keys %h;
        $second{$_}++ for keys %h;
        my $ok = len(keys %first) == len(keys %second);
        for my $k (keys %first) {
            $ok = 0 unless exists $second{$k};
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iterating during build via push works ─────────────────────────

#[test]
fn iterate_after_each_insert() {
    let code = r#"
        my %h;
        my @after_each_insert;
        for my $k ("a", "b", "c") {
            $h{$k} = 1;
            push @after_each_insert, len(keys %h);
        }
        join(",", @after_each_insert) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Delete during iteration ──────────────────────────────────────

#[test]
fn delete_during_iteration_completes_safely() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        # Collect keys first, then delete some.
        my @to_delete = grep { $h{$_} > 2 } keys %h;
        delete $h{$_} for @to_delete;
        (exists $h{a} && exists $h{b}
            && !exists $h{c} && !exists $h{d}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort by value then iterate ───────────────────────────────────

#[test]
fn sort_by_value_descending() {
    let code = r#"
        my %scores = (alice => 90, bob => 75, carol => 85);
        my @ranked = sort { $scores{$_1} <=> $scores{$_0} } keys %scores;
        join(",", @ranked) eq "alice,carol,bob" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Stable sort by composite key ─────────────────────────────────

#[test]
fn sort_by_value_with_tiebreak() {
    let code = r#"
        my %h = (alice => 80, bob => 80, carol => 85);
        my @ranked = sort {
            ($h{$_1} <=> $h{$_0}) || ($_0 cmp $_1)
        } keys %h;
        # carol(85), alice(80), bob(80) -- alice before bob alphabetically.
        join(",", @ranked) eq "carol,alice,bob" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Convert hash to array-of-pairs ───────────────────────────────

#[test]
fn convert_hash_to_array_of_pairs() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @pairs;
        for my $k (sort { _0 cmp _1 } keys %h) {
            push @pairs, [$k, $h{$k}];
        }
        ($pairs[0]->[0] eq "a"
            && $pairs[0]->[1] == 1
            && $pairs[2]->[0] eq "c"
            && $pairs[2]->[1] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Convert array-of-pairs to hash ───────────────────────────────

#[test]
fn convert_array_of_pairs_to_hash() {
    let code = r#"
        my @pairs = (["a", 1], ["b", 2], ["c", 3]);
        my %h;
        for my $p (@pairs) {
            $h{$p->[0]} = $p->[1];
        }
        ($h{a} == 1 && $h{c} == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iteration sum over hash values ───────────────────────────────

#[test]
fn iteration_sum_of_values() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30, d => 40);
        my $total = 0;
        $total += $h{$_} for keys %h;
        $total == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sum_values_directly() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        sum(values %h) == 60 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iteration with running max ───────────────────────────────────

#[test]
fn find_max_key_via_iteration() {
    let code = r#"
        my %h = (alice => 80, bob => 95, carol => 70);
        my $best = "";
        my $best_score = -1;
        for my $k (keys %h) {
            if ($h{$k} > $best_score) {
                $best_score = $h{$k};
                $best = $k;
            }
        }
        $best eq "bob" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iteration over hash of arrayrefs ─────────────────────────────

#[test]
fn iterate_hash_of_arrayrefs_total_length() {
    let code = r#"
        my %h = (
            fruits => ["apple", "banana"],
            colors => ["red", "green", "blue"],
            nums   => [1, 2, 3, 4],
        );
        my $total = 0;
        for my $k (keys %h) {
            $total += len(@{$h{$k}});
        }
        $total == 9 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash slice via qw + iteration ────────────────────────────────

#[test]
fn slice_then_grep_present() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        my @vals = @h{qw(a c e)};
        my @defined = grep { defined($_) } @vals;
        len(@defined) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iteration consistency: keys vs values pairwise ────────────────

#[test]
fn keys_and_values_pairwise_same_index_safe_only_if_sorted() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        # Direct keys/values pairs are NOT order-guaranteed in stryke.
        # Use sort+lookup pattern instead.
        my @keys = sort { _0 cmp _1 } keys %h;
        my @vals = map { $h{$_} } @keys;
        join(",", @vals) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash size limits + iteration sanity ──────────────────────────

#[test]
fn iteration_over_10k_hash() {
    let code = r#"
        my %h;
        $h{"k$_"} = $_ for (1:10000);
        my $count = 0;
        $count++ for keys %h;
        $count == 10000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── For-loop captures one snapshot ────────────────────────────────

#[test]
fn for_loop_uses_keys_at_start() {
    let code = r#"
        my %h = (a => 1, b => 2);
        my @collected;
        for my $k (keys %h) {
            push @collected, $k;
        }
        len(@collected) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
