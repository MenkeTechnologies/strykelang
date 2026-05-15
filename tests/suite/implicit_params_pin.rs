//! Implicit-parameter pins. World-first feature surface:
//!
//!   _0 / _1 / _N         — positional block parameters (0-indexed)
//!   _                    — bareword topic (= $_)
//!   _<, _<<, _<<<, ...    — outer-topic chain, 1..5 frames up
//!   _1<<, _2<<<<, ...     — Nth positional N frames up
//!
//! No other language exposes this layered topic system. Pins below
//! lock the surface so a parser refactor can't silently break it.

use crate::common::*;

// ── _0 / _1 in sort / reduce ──────────────────────────────────────────

#[test]
fn underscore_zero_one_sort_ascending() {
    let code = r#"
        my @s = sort { _0 <=> _1 } (3, 1, 4, 1, 5, 9, 2, 6);
        join(",", @s) eq "1,1,2,3,4,5,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_zero_one_sort_descending() {
    let code = r#"
        my @s = sort { _1 <=> _0 } (3, 1, 4, 1, 5, 9);
        join(",", @s) eq "9,5,4,3,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_zero_one_reduce_product() {
    let code = r#"
        reduce { _0 * _1 } 1, (1:6)
    "#;
    assert_eq!(eval_int(code), 720); // 6!
}

#[test]
fn underscore_zero_one_reduce_max() {
    let code = r#"
        reduce { _0 > _1 ? _0 : _1 } 0, (3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5)
    "#;
    assert_eq!(eval_int(code), 9);
}

#[test]
fn sort_by_hash_field_via_underscore_zero_one() {
    let code = r#"
        my @rows = (
            +{ name => "carol", age => 30 },
            +{ name => "alice", age => 25 },
            +{ name => "bob",   age => 28 },
        );
        my @sorted = sort { _0->{age} <=> _1->{age} } @rows;
        ($sorted[0]->{name} eq "alice"
            && $sorted[1]->{name} eq "bob"
            && $sorted[2]->{name} eq "carol") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Bareword `_` topic ────────────────────────────────────────────────

#[test]
fn bareword_underscore_in_map() {
    let code = r#"
        my @r = map { _ * 2 } (1:5);
        join(",", @r) eq "2,4,6,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bareword_underscore_in_grep() {
    let code = r#"
        my @r = grep { _ % 2 == 0 } (1:10);
        join(",", @r) eq "2,4,6,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn bareword_underscore_equals_dollar_underscore() {
    let code = r#"
        my @r = map { _ == $_ ? 1 : 0 } (1:5);
        sum(@r) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── _0 / _1 / _N as multi-positional callback bindings ────────────────

#[test]
fn three_arg_callback_binds_underscore_zero_one_two() {
    // When a closure receives 3 args, _0/_1/_2 each pick a positional.
    // Use a namespaced fn name — bare `apply` is a stryke builtin.
    let code = r#"
        fn Demo::Pin::apply($f) { $f->(10, 20, 30) }
        Demo::Pin::apply(sub { _0 + _1 * _2 })
    "#;
    assert_eq!(eval_int(code), 10 + 20 * 30);
}

// ── Outer-topic chain `_<` (one frame up) ─────────────────────────────

#[test]
fn outer_topic_one_level_in_nested_map() {
    let code = r#"
        my @r = map { (1:3) |> map { _ + _< } } (10, 20, 30);
        # Map-of-map flattens in scalar list context (Perl rule).
        scalar(@r) == 9
            && join(",", @r) eq "11,12,13,21,22,23,31,32,33" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn outer_topic_with_pipe_forward_grep() {
    let code = r#"
        my @r = map {
            my $threshold = _;
            (1:10) |> grep { _ >= _< }
        } (3, 7);
        # threshold=3 → 3..10 (8 items); threshold=7 → 7..10 (4 items)
        scalar(@r) == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Two-frame outer-topic chain `_<<` ────────────────────────────────

#[test]
fn outer_topic_two_levels_sums_correctly() {
    // Three nested maps. Innermost reads outer _<< as the topmost
    // frame's topic. Sum over the cartesian product (1,2) × (10,20,30)
    // × (100,200) of (i + j + k):
    //   for each (k, j, i): sum_i(i+j+k) = 2*(j+k) + 3
    //   sum_j = 2*60 + 6k + 9 = 120 + 6k + 9 = 129 + 6k
    //   sum_k = 2*129 + 6*300 = 258 + 1800 = 2058
    let code = r#"
        my @r = map {
            map {
                map { _ + _< + _<< } (1, 2)
            } (10, 20, 30)
        } (100, 200);
        scalar(@r) == 12 && sum(@r) == 2058 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Topic captured by closure does not leak across iterations ─────────

#[test]
fn topic_does_not_leak_across_outer_iterations() {
    // Each outer iteration should see its OWN topic, not a shared one.
    let code = r#"
        my @sums;
        for my $base (10, 20, 30) {
            my @inner = map { _ * $base } (1, 2, 3);
            push @sums, sum(@inner);
        }
        # 10*(1+2+3) + 20*(...) + 30*(...) = 60 + 120 + 180 = 360
        sum(@sums) == 360 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── _0 inside `~>` thread macro stages ────────────────────────────────

#[test]
fn thread_macro_block_stage_uses_underscore() {
    let code = r#"
        my @r = ~> (1:5) map { _ * 10 } fi { _ > 20 } sort { _0 <=> _1 };
        # *10 → (10,20,30,40,50); >20 → (30,40,50); sorted → same.
        join(",", @r) eq "30,40,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pipe-forward block stages use the same _ ─────────────────────────

#[test]
fn pipe_forward_block_stages_use_underscore() {
    let code = r#"
        my $total = (1:100)
            |> map { _ * _ }
            |> grep { _ % 2 == 0 }
            |> sum;
        $total == 171700 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Topic in custom callback functions ────────────────────────────────

#[test]
fn callback_with_underscore_topic_works_in_grep() {
    // Namespaced — `pred` is a stryke builtin alias.
    let code = r#"
        fn Demo::Pin::is_big($x) { $x > 5 }
        my @r = grep { Demo::Pin::is_big(_) } (1:10);
        join(",", @r) eq "6,7,8,9,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Edge: empty input → empty output, no _ binding crash ─────────────

#[test]
fn map_over_empty_array_produces_empty() {
    let code = r#"
        my @empty;
        my @r = map { _ * 2 } @empty;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_over_one_element_is_identity() {
    let code = r#"
        my @r = sort { _0 <=> _1 } (42);
        ($r[0] == 42 && len(@r) == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── _0 / _1 with negative-comparison sort ─────────────────────────────

#[test]
fn underscore_string_compare_sort() {
    let code = r#"
        my @r = sort { _0 cmp _1 } ("banana", "apple", "cherry");
        join(",", @r) eq "apple,banana,cherry" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Cross-feature: implicit params + sketches ─────────────────────────

#[test]
fn map_with_implicit_param_into_bloom() {
    let code = r#"
        my $b = bloom_filter(1000, 0.01);
        bloom_add($b, "user:$_") for (1:100);
        my @r = grep { bloom_contains($b, "user:$_") } (1:100);
        len(@r) == 100 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
