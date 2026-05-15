//! Sort surface pins. sort/psort/sort_by + comparator forms.

use crate::common::*;

// ── Default sort (lexical) ──────────────────────────────────────────

#[test]
fn default_sort_is_lexical() {
    let code = r#"
        my @r = sort (10, 2, 30, 1, 20);
        # Lexical: "1","10","2","20","30".
        join(",", @r) eq "1,10,2,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_sort_via_spaceship() {
    let code = r#"
        my @r = sort { _0 <=> _1 } (10, 2, 30, 1, 20);
        join(",", @r) eq "1,2,10,20,30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_sort_via_cmp() {
    let code = r#"
        my @r = sort { _0 cmp _1 } ("delta", "alpha", "charlie", "bravo");
        join(",", @r) eq "alpha,bravo,charlie,delta" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort stability ──────────────────────────────────────────────────

#[test]
fn sort_is_stable_for_equal_keys() {
    let code = r#"
        my @items = (
            +{ key => 1, name => "first"  },
            +{ key => 2, name => "second" },
            +{ key => 1, name => "third"  },
            +{ key => 2, name => "fourth" },
        );
        my @sorted = sort { _0->{key} <=> _1->{key} } @items;
        # Among equal keys, original order preserved: first, third, second, fourth.
        ($sorted[0]->{name} eq "first"
            && $sorted[1]->{name} eq "third"
            && $sorted[2]->{name} eq "second"
            && $sorted[3]->{name} eq "fourth") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort by single field ────────────────────────────────────────────

#[test]
fn sort_by_age_field() {
    let code = r#"
        my @users = (
            +{ name => "carol", age => 35 },
            +{ name => "alice", age => 25 },
            +{ name => "bob",   age => 28 },
        );
        my @sorted = sort { _0->{age} <=> _1->{age} } @users;
        $sorted[0]->{name} eq "alice" && $sorted[2]->{name} eq "carol" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-field tiebreak ────────────────────────────────────────────

#[test]
fn multi_field_sort_tiebreak() {
    let code = r#"
        my @users = (
            +{ dept => "eng", name => "bob"   },
            +{ dept => "qa",  name => "alice" },
            +{ dept => "eng", name => "alice" },
            +{ dept => "qa",  name => "bob"   },
        );
        my @sorted = sort {
            ($_0->{dept} cmp $_1->{dept}) || ($_0->{name} cmp $_1->{name})
        } @users;
        # eng+alice, eng+bob, qa+alice, qa+bob.
        ($sorted[0]->{dept} eq "eng" && $sorted[0]->{name} eq "alice"
            && $sorted[1]->{dept} eq "eng" && $sorted[1]->{name} eq "bob"
            && $sorted[2]->{dept} eq "qa"  && $sorted[2]->{name} eq "alice"
            && $sorted[3]->{dept} eq "qa"  && $sorted[3]->{name} eq "bob") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort descending ─────────────────────────────────────────────────

#[test]
fn sort_descending_via_swapped_args() {
    let code = r#"
        my @r = sort { _1 <=> _0 } (3, 1, 4, 1, 5, 9, 2, 6);
        join(",", @r) eq "9,6,5,4,3,2,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort empty / single / pair ─────────────────────────────────────

#[test]
fn sort_empty_array_yields_empty() {
    let code = r#"
        my @empty;
        my @r = sort @empty;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_single_element_unchanged() {
    let code = r#"
        my @r = sort (42);
        (len(@r) == 1 && $r[0] == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_pair_swaps_when_needed() {
    let code = r#"
        my @r = sort { _0 <=> _1 } (99, 1);
        $r[0] == 1 && $r[1] == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── psort (parallel sort) ──────────────────────────────────────────

#[test]
fn psort_matches_sequential_sort_for_small_input() {
    let code = r#"
        my @input = (3, 1, 4, 1, 5, 9, 2, 6, 5, 3);
        my @par = psort { _0 <=> _1 } @input;
        my @seq = sort  { _0 <=> _1 } @input;
        join(",", @par) eq join(",", @seq) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn psort_large_input_correctness() {
    let code = r#"
        my @input = reverse (1:1000);
        my @sorted = psort { _0 <=> _1 } @input;
        ($sorted[0] == 1 && $sorted[999] == 1000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort followed by slice for top-N ───────────────────────────────

#[test]
fn top_3_via_sort_descending_plus_slice() {
    let code = r#"
        my @nums = (5, 2, 8, 1, 9, 3, 7, 4, 6);
        my @sorted = sort { _1 <=> _0 } @nums;
        my @top3 = @sorted[0:2];
        join(",", @top3) eq "9,8,7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort hash keys ──────────────────────────────────────────────────

#[test]
fn sort_hash_keys_alpha() {
    let code = r#"
        my %h = (banana => 1, apple => 2, cherry => 3);
        my @keys = sort { _0 cmp _1 } keys %h;
        join(",", @keys) eq "apple,banana,cherry" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_hash_keys_by_value() {
    let code = r#"
        my %h = (a => 3, b => 1, c => 2);
        my @keys = sort { $h{$_0} <=> $h{$_1} } keys %h;
        # b(1), c(2), a(3).
        join(",", @keys) eq "b,c,a" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort over composite array structure ────────────────────────────

#[test]
fn sort_array_of_arrayrefs_by_first_element() {
    let code = r#"
        my @rows = ([3, "c"], [1, "a"], [2, "b"]);
        my @sorted = sort { $_0->[0] <=> $_1->[0] } @rows;
        ($sorted[0]->[1] eq "a" && $sorted[2]->[1] eq "c") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort with side-effect-free comparator ──────────────────────────

#[test]
fn sort_does_not_mutate_input() {
    let code = r#"
        my @input = (3, 1, 4, 1, 5);
        my $orig = join(",", @input);
        my @_sorted = sort { _0 <=> _1 } @input;
        join(",", @input) eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort with -N descending (sort numerically descending) ─────────

#[test]
fn negate_sort_for_descending() {
    let code = r#"
        my @r = sort { -($_0 <=> $_1) } (3, 1, 4, 1, 5);
        join(",", @r) eq "5,4,3,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Schwartzian transform pattern ──────────────────────────────────

#[test]
fn schwartzian_transform_pattern() {
    let code = r#"
        my @words = ("delta", "alpha", "charlie", "bravo");
        # Sort by length, then alphabetically.
        my @sorted = map  { $_->[1] }
                     sort { $_0->[0] <=> $_1->[0] || $_0->[1] cmp $_1->[1] }
                     map  { [len($_), $_] }
                     @words;
        join(",", @sorted) eq "alpha,bravo,delta,charlie" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort 100 random elements correctness ───────────────────────────

#[test]
fn sort_hundred_random_elements_correct() {
    let code = r#"
        my @input;
        my $seed = 12345;
        for my $i (1:100) {
            $seed = ($seed * 1103515245 + 12345) % 2147483648;
            push @input, $seed % 1000;
        }
        my @sorted = sort { _0 <=> _1 } @input;
        # Verify monotonicity.
        my $ok = 1;
        for my $i (1:99) {
            if ($sorted[$i] < $sorted[$i - 1]) {
                $ok = 0;
                last;
            }
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
