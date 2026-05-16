//! grep/map/sort/reduce chain composition pins.

use crate::common::*;

// ── Two-stage chain ──────────────────────────────────────────────

#[test]
fn map_then_grep() {
    let code = r#"
        my @r = grep { _ > 10 } map { _ * 5 } (1, 2, 3, 4);
        # *5 → 5, 10, 15, 20; > 10 → 15, 20.
        join(",", @r) eq "15,20" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn grep_then_map() {
    let code = r#"
        my @r = map { _ * 5 } grep { _ % 2 == 0 } (1, 2, 3, 4, 5);
        # evens → 2, 4; *5 → 10, 20.
        join(",", @r) eq "10,20" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Three-stage chain ───────────────────────────────────────────

#[test]
fn map_grep_sort_chain() {
    let code = r#"
        my @r = sort { _0 <=> _1 }
                grep { _ > 5 }
                map { _ * 2 }
                (1, 2, 3, 4, 5, 6);
        # *2 → 2, 4, 6, 8, 10, 12; > 5 → 6, 8, 10, 12; sorted same.
        join(",", @r) eq "6,8,10,12" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Chain with reduce ───────────────────────────────────────────

#[test]
fn map_grep_reduce_chain() {
    let code = r#"
        my $r = reduce { _0 + _1 } 0,
                grep { _ > 10 }
                map { _ * _ }
                (1:5);
        # squares 1..5: 1, 4, 9, 16, 25; > 10 → 16, 25; sum = 41.
        $r == 41 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pipeline equivalent via pipe-forward ─────────────────────────

#[test]
fn pipe_forward_chain_equivalent() {
    let code = r#"
        my $r = (1:10)
            |> map { _ * _ }
            |> grep { _ % 2 == 0 }
            |> sum;
        # squares: 1,4,9,16,25,36,49,64,81,100; even: 4,16,36,64,100; sum=220.
        $r == 220 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large input ─────────────────────────────────────────────────

#[test]
fn large_input_pipeline_correctness() {
    let code = r#"
        my $r = sum(grep { _ > 5000 } map { _ * _ } (1:100));
        # squares of 71..100 are > 5000 (71^2 = 5041).
        # sum of squares 71..100 = sum_1..100 - sum_1..70.
        # sum_1..N = N(N+1)(2N+1)/6.
        my $expected = (100*101*201)/6 - (70*71*141)/6;
        $r == $expected ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep over hashref values ────────────────────────────────────

#[test]
fn grep_over_hashref_values() {
    let code = r#"
        my $h = +{ a => 5, b => 15, c => 25, d => 10 };
        my @big = grep { _ > 10 } values %$h;
        len(@big) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort then slice top N ──────────────────────────────────────

#[test]
fn top_three_via_sort_slice() {
    let code = r#"
        my @scores = (75, 82, 91, 68, 88, 95, 70);
        my @sorted = sort { _1 <=> _0 } @scores;
        my @top = @sorted[0:2];
        join(",", @top) eq "95,91,88" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep with regex ─────────────────────────────────────────────

#[test]
fn grep_regex_filter() {
    let code = r#"
        my @input = ("alice", "bob", "carol", "anna", "alex");
        my @starts_a = sort grep { /^a/ } @input;
        join(",", @starts_a) eq "alex,alice,anna" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── map with sprintf format ──────────────────────────────────────

#[test]
fn map_with_sprintf() {
    let code = r#"
        my @r = map { sprintf("%03d", _) } (1, 10, 100);
        join(",", @r) eq "001,010,100" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── map returning multiple values per input ──────────────────────

#[test]
fn map_explosion_each_in_to_two_out() {
    let code = r#"
        my @r = map { ($_, $_ * 2) } (1, 2, 3);
        # Each input -> 2 outputs.
        len(@r) == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep + count via scalar context ──────────────────────────────

#[test]
fn count_filtered_via_scalar_grep() {
    let code = r#"
        my $n = scalar(grep { _ > 50 } (10, 60, 30, 80, 25, 100));
        $n == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Schwartzian transform ──────────────────────────────────────

#[test]
fn schwartzian_transform() {
    let code = r#"
        my @words = ("foo", "longest_one", "ab", "med");
        my @by_len = map  { $_->[1] }
                     sort { $_0->[0] <=> $_1->[0] }
                     map  { [len($_), $_] }
                     @words;
        # By length: ab(2), foo(3), med(3), longest_one(11).
        $by_len[0] eq "ab" && $by_len[3] eq "longest_one" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep idiom: filter undef ─────────────────────────────────────

#[test]
fn grep_filters_undef() {
    let code = r#"
        my @input = (1, undef, 2, undef, 3);
        my @defined = grep { defined($_) } @input;
        join(",", @defined) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep + map round-trip ───────────────────────────────────────

#[test]
fn grep_map_inverse_for_identity() {
    let code = r#"
        my @input = (1, 2, 3, 4, 5);
        my @id = map { _ } grep { 1 } @input;
        join(",", @id) eq "1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Chain into HLL sketch ──────────────────────────────────────

#[test]
fn chain_into_hll() {
    let code = r#"
        my @input = map { "user_$_" } (1:1000);
        my $hll = hll(14);
        hll_add($hll, $_) for grep { /^user_/ } @input;
        my $est = hll_count($hll);
        ($est >= 950 && $est <= 1050) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep over results of map of fn calls ────────────────────────

#[test]
fn grep_over_fn_call_results() {
    let code = r#"
        fn Demo::GM::triple($n) { $n * 3 }
        my @r = grep { _ > 10 } map { Demo::GM::triple($_) } (1, 2, 3, 4, 5);
        # *3 → 3, 6, 9, 12, 15; > 10 → 12, 15.
        join(",", @r) eq "12,15" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pipeline with hash building ─────────────────────────────────

#[test]
fn pipeline_builds_hash() {
    let code = r#"
        my @entries = ("alice=30", "bob=25", "carol=42");
        my %parsed;
        for my $kv (@entries) {
            my ($k, $v) = split /=/, $kv;
            $parsed{$k} = $v + 0;
        }
        ($parsed{alice} == 30 && $parsed{bob} == 25) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty input through chain returns empty ──────────────────────

#[test]
fn empty_chain_returns_empty() {
    let code = r#"
        my @empty;
        my @r = grep { _ > 0 } map { _ * 2 } @empty;
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reduce on chained pipeline ───────────────────────────────────

#[test]
fn reduce_product_after_filter() {
    let code = r#"
        my $r = reduce { _0 * _1 } 1, grep { _ > 0 } (1, -2, 3, -4, 5);
        # Positives: 1, 3, 5 → product = 15.
        $r == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep with closure ───────────────────────────────────────────

#[test]
fn grep_with_explicit_closure_callback() {
    let code = r#"
        my $is_prime_ish = sub { $_[0] > 1 && $_[0] % 2 == 1 };
        my @r = grep { $is_prime_ish->($_) } (1:10);
        # Odd > 1: 3, 5, 7, 9.
        join(",", @r) eq "3,5,7,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 1000-element pipeline ───────────────────────────────────────

#[test]
fn pipeline_1000_elements_consistent() {
    let code = r#"
        my $r1 = sum(map { _ * 2 } grep { _ % 3 == 0 } (1:1000));
        # Multiples of 3 up to 1000: 333 numbers (3, 6, ..., 999).
        # Sum: 333 * (3 + 999) / 2 = 166_833. Doubled: 333_666.
        $r1 == 333_666 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep + map order is critical ────────────────────────────────

#[test]
fn map_grep_vs_grep_map_differ() {
    let code = r#"
        # map first then grep: differs from grep first then map.
        my @a = grep { _ > 5 } map { _ + 3 } (1, 2, 3);
        my @b = map { _ + 3 } grep { _ > 5 } (1, 2, 3);
        # @a: +3 → 4,5,6 → >5 → 6. @b: >5 → (none) → +3 → (none).
        ($a[0] == 6 && len(@a) == 1 && len(@b) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── thread macro composition ─────────────────────────────────────

#[test]
fn thread_macro_chain_equivalent_to_pipe() {
    let code = r#"
        my @r = ~> (1:10) map { _ * 2 } grep { _ > 10 };
        # *2 → 2..20; > 10 → 12,14,16,18,20.
        join(",", @r) eq "12,14,16,18,20" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── uniq + sort chain ────────────────────────────────────────────

#[test]
fn uniq_then_sort() {
    let code = r#"
        my @r = sort { _0 cmp _1 } uniq("c", "a", "b", "a", "c", "d");
        join(",", @r) eq "a,b,c,d" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reverse + sort ──────────────────────────────────────────────

#[test]
fn reverse_after_sort() {
    let code = r#"
        my @r = reverse sort (3, 1, 4, 1, 5);
        join(",", @r) eq "5,4,3,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
