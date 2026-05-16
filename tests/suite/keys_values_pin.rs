//! keys / values builtin pins. Complement hash_iteration_order_pin.

use crate::common::*;

// ── keys on bare hash ──────────────────────────────────────────────

#[test]
fn keys_returns_all_keys() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @ks = sort { _0 cmp _1 } keys %h;
        join(",", @ks) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn keys_count_matches_size() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        len(keys %h) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn keys_empty_hash_returns_empty() {
    let code = r#"
        my %empty;
        len(keys %empty) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys on hashref ────────────────────────────────────────────────

#[test]
fn keys_on_hashref_via_deref() {
    let code = r#"
        my $h = +{ a => 1, b => 2, c => 3 };
        my @ks = sort { _0 cmp _1 } keys %$h;
        join(",", @ks) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn keys_on_hashref_via_curly_deref() {
    let code = r#"
        my $h = +{ x => 1, y => 2 };
        my @ks = sort { _0 cmp _1 } keys %{$h};
        join(",", @ks) eq "x,y" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── values on bare hash ─────────────────────────────────────────────

#[test]
fn values_returns_all_values() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @vs = sort { _0 <=> _1 } values %h;
        join(",", @vs) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn values_sum_correctly() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30, d => 40);
        sum(values %h) == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── values on hashref ───────────────────────────────────────────────

#[test]
fn values_on_hashref_via_deref() {
    let code = r#"
        my $h = +{ a => 1, b => 2, c => 3 };
        my @vs = sort { _0 <=> _1 } values %$h;
        join(",", @vs) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys + map composition ─────────────────────────────────────────

#[test]
fn map_over_keys_yields_transformed() {
    let code = r#"
        my %h = (alpha => 1, beta => 2, gamma => 3);
        my @uc = sort map { uc($_) } keys %h;
        join(",", @uc) eq "ALPHA,BETA,GAMMA" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn map_over_values_doubles_each() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @doubled = sort { _0 <=> _1 } map { _ * 2 } values %h;
        join(",", @doubled) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys + grep filter ─────────────────────────────────────────────

#[test]
fn grep_over_keys_filters() {
    let code = r#"
        my %h = (alpha => 1, beta => 2, animal => 3, banana => 4);
        my @starts_a = sort grep { /^a/ } keys %h;
        join(",", @starts_a) eq "alpha,animal" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys after delete ──────────────────────────────────────────────

#[test]
fn keys_count_decreases_after_delete() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        my $before = len(keys %h);
        delete $h{b};
        my $after = len(keys %h);
        ($before == 4 && $after == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── values after delete ────────────────────────────────────────────

#[test]
fn values_count_decreases_after_delete() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        delete $h{b};
        my $sum = sum(values %h);
        $sum == 40 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large hash ─────────────────────────────────────────────────────

#[test]
fn keys_large_hash() {
    let code = r#"
        my %h;
        for my $i (1:1000) {
            $h{"k$i"} = $i;
        }
        len(keys %h) == 1000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn values_large_hash_sum() {
    let code = r#"
        my %h;
        for my $i (1:100) {
            $h{"k$i"} = $i;
        }
        sum(values %h) == 5050 ? 1 : 0   # 1+2+...+100
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash of hashrefs ───────────────────────────────────────────────

#[test]
fn keys_of_nested_hash() {
    let code = r#"
        my %h = (
            a => +{ x => 1 },
            b => +{ y => 2 },
        );
        my @ks = sort { _0 cmp _1 } keys %h;
        join(",", @ks) eq "a,b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nested_keys_via_double_iteration() {
    let code = r#"
        my %h = (
            a => +{ x => 1, y => 2 },
            b => +{ z => 3 },
        );
        my @all_inner;
        for my $k (sort { _0 cmp _1 } keys %h) {
            push @all_inner, sort { _0 cmp _1 } keys %{$h{$k}};
        }
        join(",", @all_inner) eq "x,y,z" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── values transformation via map+grep ────────────────────────────

#[test]
fn map_grep_chain_on_values() {
    let code = r#"
        my %h = (a => 10, b => 25, c => 5, d => 50);
        my @big_doubled = sort { _0 <=> _1 }
                          map { _ * 2 }
                          grep { _ > 10 }
                          values %h;
        # >10: 25, 50; doubled: 50, 100.
        join(",", @big_doubled) eq "50,100" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exists matches keys ────────────────────────────────────────────

#[test]
fn exists_for_each_key() {
    let code = r#"
        my %h = (a => 1, b => 2);
        my $ok = 1;
        for my $k (keys %h) {
            $ok = 0 unless exists($h{$k});
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys returns scalar in scalar context (bucket count) ─────────

#[test]
fn keys_via_scalar_returns_count() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        scalar(keys %h) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── values mutating original via deref ─────────────────────────────

#[test]
fn modify_value_via_key_lookup() {
    let code = r#"
        my %h = (a => 1, b => 2);
        for my $k (keys %h) {
            $h{$k} = $h{$k} * 10;
        }
        ($h{a} == 10 && $h{b} == 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Convert hash to sorted list of pairs ──────────────────────────

#[test]
fn convert_hash_to_sorted_pairs() {
    let code = r#"
        my %h = (b => 2, a => 1, c => 3);
        my @pairs;
        for my $k (sort { _0 cmp _1 } keys %h) {
            push @pairs, "$k=$h{$k}";
        }
        join(",", @pairs) eq "a=1,b=2,c=3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort by value desc ─────────────────────────────────────────────

#[test]
fn sort_keys_by_value_descending() {
    let code = r#"
        my %scores = (alice => 80, bob => 95, carol => 70);
        my @ranked = sort { $scores{$_1} <=> $scores{$_0} } keys %scores;
        join(",", @ranked) eq "bob,alice,carol" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Find max-value key via reduce-like pattern ───────────────────

#[test]
fn find_max_value_key() {
    let code = r#"
        my %h = (a => 10, b => 50, c => 30, d => 20);
        my @ranked = sort { $h{$_1} <=> $h{$_0} } keys %h;
        my $best = $ranked[0];
        $best eq "b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── min value via iteration ─────────────────────────────────────────

#[test]
fn min_value_via_keys_iteration() {
    let code = r#"
        my %h = (a => 10, b => 50, c => 5, d => 20);
        min(values %h) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 10k key/value distinct count ───────────────────────────────────

#[test]
fn keys_and_values_10k_count() {
    let code = r#"
        my %h;
        $h{"k$_"} = $_ for (1:10000);
        (len(keys %h) == 10000 && len(values %h) == 10000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
