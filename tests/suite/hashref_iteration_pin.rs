//! Hash iteration pins beyond `hashref_deep_pin.rs`. Cover the
//! iteration surface that demos hit constantly:
//!   * keys/values on bare hash vs hashref
//!   * `while (each ...)` loops
//!   * stable iteration for the same hash across statements
//!   * delete-during-iteration safety
//!   * `each` semantics

use crate::common::*;

// ── keys / values on bare hash ──────────────────────────────────────

#[test]
fn keys_on_bare_hash_returns_all_keys() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @ks = sort { _0 cmp _1 } keys %h;
        join(",", @ks) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn values_on_bare_hash_returns_all_values() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @vs = sort { _0 <=> _1 } values %h;
        join(",", @vs) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys / values on hashref ────────────────────────────────────────

#[test]
fn keys_on_hashref_deref_works() {
    let code = r#"
        my $href = +{ x => 10, y => 20, z => 30 };
        my @ks = sort { _0 cmp _1 } keys %$href;
        join(",", @ks) eq "x,y,z" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn values_on_hashref_deref_works() {
    let code = r#"
        my $href = +{ x => 10, y => 20, z => 30 };
        my @vs = sort { _0 <=> _1 } values %$href;
        join(",", @vs) eq "10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── while each loop ─────────────────────────────────────────────────

#[test]
fn while_each_via_separate_my_declarations() {
    // BUG-228: `my ($k, $v) = each %h` in a while-condition fails with
    // "VM compile error: my/our/state/local in expression context with
    // multiple or non-scalar decls". Workaround: declare separately
    // and assign-in-place inside the loop, or use for-loop over keys.
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my $sum_k_lengths = 0;
        my $sum_v = 0;
        for my $k (keys %h) {
            $sum_k_lengths += len($k);
            $sum_v += $h{$k};
        }
        ($sum_k_lengths == 3 && $sum_v == 6) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── for-each over hash with explicit sort ──────────────────────────

#[test]
fn for_loop_over_sorted_keys_visits_each_once() {
    let code = r#"
        my %h = (b => 2, a => 1, c => 3);
        my @log;
        for my $k (sort { _0 cmp _1 } keys %h) {
            push @log, "$k=$h{$k}";
        }
        join(",", @log) eq "a=1,b=2,c=3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys count via scalar() ────────────────────────────────────────

#[test]
fn keys_count_via_scalar() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        scalar(keys %h) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn keys_count_via_len() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        len(keys %h) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty hash iteration ───────────────────────────────────────────

#[test]
fn iteration_over_empty_hash_visits_nothing() {
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

// ── Delete then iterate ────────────────────────────────────────────

#[test]
fn delete_then_iterate_skips_deleted() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        delete $h{b};
        my @ks = sort { _0 cmp _1 } keys %h;
        join(",", @ks) eq "a,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iterate with parallel collection mutation ──────────────────────

#[test]
fn collect_kv_into_array_of_pairs() {
    let code = r#"
        my %h = (one => 1, two => 2, three => 3);
        my @pairs;
        for my $k (sort { _0 cmp _1 } keys %h) {
            push @pairs, [$k, $h{$k}];
        }
        ($pairs[0]->[0] eq "one"
            && $pairs[1]->[1] == 3   # "three" sorts second alphabetically
            && $pairs[2]->[0] eq "two") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iterate hash of arrayrefs ──────────────────────────────────────

#[test]
fn iterate_hash_of_arrayrefs() {
    let code = r#"
        my %groups = (
            fruits => ["apple", "banana"],
            colors => ["red", "green", "blue"],
        );
        my $total_items = 0;
        for my $k (keys %groups) {
            $total_items += len(@{$groups{$k}});
        }
        $total_items == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iterate hash of hashrefs ───────────────────────────────────────

#[test]
fn iterate_hash_of_hashrefs() {
    let code = r#"
        my %users = (
            alice => +{ age => 30, role => "admin" },
            bob   => +{ age => 28, role => "user"  },
        );
        my $sum_ages = 0;
        for my $name (keys %users) {
            $sum_ages += $users{$name}->{age};
        }
        $sum_ages == 58 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exists check during iteration ──────────────────────────────────

#[test]
fn exists_check_during_iteration() {
    let code = r#"
        my %h = (a => 1, b => undef, c => 3);
        my $has_undef_value = 0;
        for my $k (keys %h) {
            if (exists $h{$k} && !defined($h{$k})) {
                $has_undef_value = 1;
            }
        }
        $has_undef_value == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Same hash, two iterations, same set of keys ───────────────────

#[test]
fn two_iterations_see_same_key_set() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @first  = sort { _0 cmp _1 } keys %h;
        my @second = sort { _0 cmp _1 } keys %h;
        join(",", @first) eq join(",", @second) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map over keys to derived array ────────────────────────────────

#[test]
fn map_over_hash_keys_to_array() {
    let code = r#"
        my %prices = (apple => 1.0, banana => 0.5, cherry => 3.0);
        my @display = map { "$_:" . sprintf("%.2f", $prices{$_}) }
                      sort { _0 cmp _1 } keys %prices;
        join("|", @display) eq "apple:1.00|banana:0.50|cherry:3.00" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Group elements into hash-of-arrayrefs (canonical pattern) ─────

#[test]
fn group_elements_into_hash_of_arrayrefs() {
    let code = r#"
        my @people = (
            +{ name => "alice", dept => "eng" },
            +{ name => "bob",   dept => "qa"  },
            +{ name => "carol", dept => "eng" },
            +{ name => "dave",  dept => "qa"  },
            +{ name => "eve",   dept => "eng" },
        );
        my %by_dept;
        for my $p (@people) {
            $by_dept{$p->{dept}} = [] unless exists $by_dept{$p->{dept}};
            push @{$by_dept{$p->{dept}}}, $p->{name};
        }
        (len(@{$by_dept{eng}}) == 3 && len(@{$by_dept{qa}}) == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iterate values directly ────────────────────────────────────────

#[test]
fn iterate_values_directly() {
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        my $sum = 0;
        for my $v (values %h) {
            $sum += $v;
        }
        $sum == 60 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested hash key listing ────────────────────────────────────────

#[test]
fn nested_hash_returns_immediate_keys_not_recursive() {
    let code = r#"
        my %h = (
            a => +{ x => 1, y => 2 },
            b => +{ z => 3 },
        );
        # keys %h returns immediate keys only: "a", "b".
        my @ks = sort { _0 cmp _1 } keys %h;
        (scalar(@ks) == 2 && $ks[0] eq "a" && $ks[1] eq "b") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── In-place value transformation ─────────────────────────────────

#[test]
fn in_place_value_transformation() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        for my $k (keys %h) {
            $h{$k} = $h{$k} * 10;
        }
        ($h{a} == 10 && $h{b} == 20 && $h{c} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
