//! Hash-slice pins. `@h{LIST}` returns values; `%h{LIST}` returns
//! key-value pairs (Perl 5.20+). `delete @h{LIST}` removes batch.

use crate::common::*;

// ── Value slice `@h{KEYS}` ─────────────────────────────────────────

#[test]
fn hash_value_slice_returns_values() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        my @v = @h{qw(a c)};
        join(",", @v) eq "1,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_value_slice_with_array_keys_returns_empty_buggy() {
    // BUG-235: `@h{@arrayvar}` returns a single empty element instead
    // of the values at those keys. `@h{qw(...)}` and `@h{"a","b"}`
    // both work; only the array-variable form is broken.
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        my @keys = ("a", "c");
        my @v = @h{@keys};
        len(@v) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_value_slice_undef_for_missing_keys() {
    let code = r#"
        my %h = (a => 1, b => 2);
        my @v = @h{qw(a xxx)};
        ($v[0] == 1 && !defined($v[1])) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── batch delete via slice ─────────────────────────────────────────

#[test]
fn delete_per_key_workaround_for_batch_delete() {
    // BUG-236: `delete @h{qw(...)}` slice form errors with
    // "delete requires hash or array element". Workaround:
    // loop over keys and delete each individually.
    let code = r#"
        my %h = (a => 1, b => 2, c => 3, d => 4);
        for my $k (qw(a c)) {
            delete $h{$k};
        }
        (exists $h{b} && exists $h{d}
            && !exists $h{a} && !exists $h{c}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_per_key_returns_value() {
    // BUG-236 workaround: collect deleted values via per-key delete.
    let code = r#"
        my %h = (a => 10, b => 20, c => 30);
        my @removed;
        for my $k (qw(a c)) {
            push @removed, delete $h{$k};
        }
        join(",", @removed) eq "10,30" && !exists $h{a} ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exists across slice ────────────────────────────────────────────

#[test]
fn exists_check_per_key() {
    let code = r#"
        my %h = (a => 1, b => 2);
        my @r;
        for my $k (qw(a b c)) {
            push @r, exists($h{$k}) ? 1 : 0;
        }
        join(",", @r) eq "1,1,0" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice assignment ──────────────────────────────────────────────

#[test]
fn slice_assignment_sets_multiple_keys() {
    let code = r#"
        my %h;
        @h{qw(a b c)} = (10, 20, 30);
        ($h{a} == 10 && $h{b} == 20 && $h{c} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn slice_assignment_overwrites_existing() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        @h{qw(a c)} = (100, 300);
        ($h{a} == 100 && $h{b} == 2 && $h{c} == 300) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash slice into pairs ──────────────────────────────────────────
// Note: `%h{LIST}` (kv slice) may not be supported in stryke;
// pin via @h{LIST} which is universal.

#[test]
fn slice_via_at_returns_just_values_not_pairs() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my @v = @h{qw(a c)};
        # @h{...} returns 2 values (not 4 kv-pairs).
        len(@v) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice as map input ─────────────────────────────────────────────

#[test]
fn slice_then_map_squares() {
    let code = r#"
        my %h = (a => 2, b => 3, c => 5);
        my @sq = map { _ * _ } @h{qw(a b c)};
        join(",", sort { _0 <=> _1 } @sq) eq "4,9,25" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice in scalar context (last element) ────────────────────────

#[test]
fn slice_in_scalar_context_returns_last_value() {
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        my $r = (@h{qw(a b c)});
        # Comma-list in scalar context returns last.
        $r == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice through hashref via arrow ────────────────────────────────

#[test]
fn arrow_dollar_one_works_per_key_in_hashref() {
    // BUG-217 (existing): @{$r}{KEYS} hash-slice-through-arrow fails.
    // Pin the workaround: pluck keys explicitly.
    let code = r#"
        my $href = +{ a => 100, b => 200, c => 300 };
        my @v = ($href->{a}, $href->{c});
        join(",", @v) eq "100,300" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice with mixed exist/missing ────────────────────────────────

#[test]
fn slice_with_mixed_keys() {
    let code = r#"
        my %h = (alpha => 1, gamma => 3);
        my @v = @h{qw(alpha beta gamma delta)};
        ($v[0] == 1 && !defined($v[1])
            && $v[2] == 3 && !defined($v[3])) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── slice + grep ──────────────────────────────────────────────────

#[test]
fn slice_then_grep_defined() {
    let code = r#"
        my %h = (a => 1, c => 3);
        my @defined = grep { defined($_) } @h{qw(a b c d)};
        len(@defined) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── slice into sum ─────────────────────────────────────────────────

#[test]
fn slice_into_sum() {
    let code = r#"
        my %prices = (apple => 1, banana => 2, cherry => 3, date => 4);
        my $total = sum(@prices{qw(apple cherry)});
        $total == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── slice with integer keys (hash with numeric keys) ──────────────

#[test]
fn slice_hash_with_integer_keys() {
    let code = r#"
        my %h = (1 => "a", 2 => "b", 3 => "c");
        my @v = @h{1, 3};
        join(",", @v) eq "a,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Re-construct hash from sliced values ──────────────────────────

#[test]
fn extract_subset_via_qw_slice() {
    // Use qw() form (the only consistent slice form per BUG-235).
    let code = r#"
        my %src = (a => 1, b => 2, c => 3, d => 4);
        my %sub;
        @sub{qw(a c)} = @src{qw(a c)};
        ($sub{a} == 1 && $sub{c} == 3 && !exists($sub{b})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash slice with computed keys ──────────────────────────────────

#[test]
fn dynamically_built_lookup_via_per_key_loop() {
    // BUG-235 workaround for dynamic keys: per-key arrow lookup.
    let code = r#"
        my %h;
        for my $i (1:10) {
            $h{"k$i"} = $i * 10;
        }
        my @keys = map { "k$_" } (3, 5, 7);
        my @v;
        for my $k (@keys) {
            push @v, $h{$k};
        }
        join(",", @v) eq "30,50,70" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty slice yields empty list ─────────────────────────────────

#[test]
fn empty_slice_via_qw_yields_empty_list() {
    let code = r#"
        my %h = (a => 1);
        my @v = @h{qw()};
        len(@v) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice from hash sorted-by-value ────────────────────────────────

#[test]
fn collect_subset_via_explicit_arrow_lookup() {
    // BUG-235: `@h{@arr}` is broken — fall back to explicit per-key
    // lookup via $h{} for dynamic key lists.
    let code = r#"
        my %scores = (alice => 90, bob => 75, carol => 85, dave => 60);
        my @top = sort { $scores{$_1} <=> $scores{$_0} } keys %scores;
        my @top3_scores;
        for my $i (0:2) {
            push @top3_scores, $scores{$top[$i]};
        }
        join(",", @top3_scores) eq "90,85,75" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
