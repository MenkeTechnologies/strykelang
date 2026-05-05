//! Behavior-pinning batch J (2026-05-04): list aggregates, chunk family,
//! sorted variants, partition, range flip-flop, conditional/dispatch idioms.

use crate::common::*;

// ── Aggregates: sum/product/min/max/mean/median/mode/stddev ─────────────────

#[test]
fn sum_one_through_ten() {
    assert_eq!(eval_int(r#"sum 1..10"#), 55);
}

#[test]
fn product_one_through_five() {
    assert_eq!(eval_int(r#"product 1..5"#), 120);
}

#[test]
fn min_variadic() {
    assert_eq!(eval_int(r#"min(7, 2, 9, 1, 5)"#), 1);
}

#[test]
fn max_variadic() {
    assert_eq!(eval_int(r#"max(7, 2, 9, 1, 5)"#), 9);
}

#[test]
fn mean_one_through_ten() {
    assert_eq!(eval_string(r#"mean(1..10)"#), "5.5");
}

#[test]
fn median_picks_middle_element() {
    assert_eq!(eval_int(r#"median(1, 5, 3, 9, 7)"#), 5);
}

#[test]
fn mode_returns_most_frequent() {
    assert_eq!(eval_int(r#"mode(1, 2, 2, 3, 2, 4)"#), 2);
}

#[test]
fn stddev_one_through_five_known_value() {
    // sqrt(2) ≈ 1.4142… (population standard deviation of 1..5).
    let s = eval_string(r#"sprintf("%.4f", stddev(1..5))"#);
    assert_eq!(s, "1.4142");
}

#[test]
fn minmax_returns_pair() {
    assert_eq!(eval_string(r#"join("/", minmax(7, 2, 9, 1, 5))"#), "1/9");
}

// ── Sorting variants ────────────────────────────────────────────────────────

#[test]
fn sorted_returns_ascending_default() {
    assert_eq!(eval_string(r#"my @r = sorted(3, 1, 2); "@r""#), "1 2 3");
}

#[test]
fn sorted_desc_returns_descending() {
    assert_eq!(
        eval_string(r#"my @r = sorted_desc(3, 1, 2); "@r""#),
        "3 2 1"
    );
}

#[test]
fn sorted_nums_orders_numerically_not_lexically() {
    assert_eq!(
        eval_string(r#"my @r = sorted_nums("30", "5", "7"); "@r""#),
        "5 7 30"
    );
}

#[test]
fn sorted_by_length_orders_strings_by_length() {
    assert_eq!(
        eval_string(r#"my @r = sorted_by_length(qw(a bbb cc dddd)); "@r""#),
        "a cc bbb dddd"
    );
}

// ── Take / drop / zip / uniq / uniq_by ──────────────────────────────────────

#[test]
fn take_list_then_count_keeps_first_n() {
    // Stryke's signature is `take(LIST, N)` — list first, count last (the
    // existing `take_first_n_from_list` test in `builtins.rs` documents
    // this). Passing `(N, LIST)` returns an empty list.
    assert_eq!(eval_string(r#"my @r = take(qw(a b c d), 2); "@r""#), "a b");
}

#[test]
fn take_n_first_signature_returns_empty_today() {
    // BUG-063: the Perl-ish `take(N, LIST)` ordering produces nothing.
    // Pin until the calling convention is unified or aliased.
    assert_eq!(eval_int(r#"my @r = take(3, 1..10); scalar @r"#), 0);
}

#[test]
fn zip_two_arrays_returns_pairs_as_arrayrefs() {
    let out = eval_string(
        r#"my @r = zip [1,2,3], ["a","b","c"];
           my $ref0 = ref($r[0]);
           "n=" . scalar(@r) . " ref=$ref0""#,
    );
    assert_eq!(out, "n=3 ref=ARRAY");
}

#[test]
fn zip_pair_contents_accessible_via_deref() {
    assert_eq!(
        eval_string(
            r#"my @r = zip [1,2,3], ["a","b","c"];
               join(",", map { join(":", @$_) } @r)"#
        ),
        "1:a,2:b,3:c"
    );
}

#[test]
fn uniq_dedupes_preserving_order() {
    assert_eq!(
        eval_string(r#"join(",", uniq(1, 2, 1, 3, 2, 4, 3))"#),
        "1,2,3,4"
    );
}

#[test]
fn uniq_by_groups_by_predicate_keeping_first() {
    assert_eq!(
        eval_string(r#"join(",", uniq_by(sub { $_ % 3 }, 1..10))"#),
        "1,2,3"
    );
}

// ── chunk family ────────────────────────────────────────────────────────────

#[test]
fn chunk_n_groups_into_runs_of_n() {
    let out = eval_string(
        r#"my @r = chunk_n(2, 1..6);
           join("|", map { join(",", @$_) } @r)"#,
    );
    assert_eq!(out, "1,2|3,4|5,6");
}

#[test]
fn chunk_while_groups_consecutive_runs() {
    // Group consecutive integers (next == prev + 1).
    assert_eq!(
        eval_string(
            r#"my @r = chunk_while(sub { $_[0] + 1 == $_[1] }, 1, 2, 3, 5, 6, 7);
               join("|", map { join(",", @$_) } @r)"#
        ),
        "1,2,3|5,6,7"
    );
}

#[test]
fn chunk_alone_returns_one_arrayref_today() {
    // BUG-058: bare `chunk(2, ...)` returns a single arrayref containing all
    // elements. Use `chunk_n` for the conventional behavior.
    assert_eq!(eval_int(r#"my @r = chunk(2, 1..6); scalar @r"#), 1);
}

// ── partition ───────────────────────────────────────────────────────────────

#[test]
fn partition_block_form_splits_into_yes_and_no() {
    // Stryke-style block form (no `sub` keyword) is the way that works.
    let out = eval_string(
        r#"my @r = partition { _ > 3 } 1..6;
           "0=[" . join(",", @{$r[0]}) . "] 1=[" . join(",", @{$r[1]}) . "]""#,
    );
    assert_eq!(out, "0=[4,5,6] 1=[1,2,3]");
}

#[test]
fn partition_sub_form_returns_empty_arrays_today() {
    // BUG-059: `partition(sub { ... }, ...)` (Perl-style) returns two empty
    // arrays. Pin until partition accepts both calling conventions.
    assert_eq!(
        eval_string(
            r#"my @r = partition(sub { $_ > 3 }, 1..6);
               "0=[" . join(",", @{$r[0]}) . "] 1=[" . join(",", @{$r[1]}) . "]""#
        ),
        "0=[] 1=[]"
    );
}

// ── Range flip-flop in scalar context is list-range today ───────────────────

#[test]
fn range_flip_flop_in_conditional_evaluates_as_list_today() {
    // BUG-060: `if ($i == 2 .. $i == 4)` should be a flip-flop that activates
    // when the left side is true and stays true until the right side is true.
    // Stryke evaluates `..` as a list-range operator: `0 .. 0` = `(0)` which
    // is a single-element non-empty list and therefore truthy.
    let out = eval_string(
        r#"my $log = "";
           for my $i (1..6) {
             $log .= "$i;" if $i == 2 .. $i == 4;
           }
           $log"#,
    );
    assert_eq!(out, "1;3;4;5;6;");
}

// ── Range with step ─────────────────────────────────────────────────────────

#[test]
fn range_with_step_yields_strided_list() {
    assert_eq!(eval_string(r#"my @a = range(1, 10, 2); "@a""#), "1 3 5 7 9");
}

#[test]
fn step_with_n_first_signature_returns_empty_today() {
    // BUG-063b: `step(N, LIST)` returns nothing. The `range(start, end,
    // step)` builtin (below) is the way to get a strided list.
    assert_eq!(eval_int(r#"my @a = step(2, 1..10); scalar @a"#), 0);
}

// ── reverse / shuffle / sample ──────────────────────────────────────────────

#[test]
fn reverse_list_in_print_with_join() {
    assert_eq!(eval_string(r#"join(",", reverse 1..5)"#), "5,4,3,2,1");
}

#[test]
fn shuffle_returns_same_count() {
    assert_eq!(eval_int(r#"my @r = shuffle 1..5; scalar @r"#), 5);
}

#[test]
fn sample_returns_requested_count() {
    assert_eq!(
        eval_int(r#"srand(1); my @r = sample(3, 1..10); scalar @r"#),
        3
    );
}

#[test]
fn sample_returns_subset_of_input() {
    let n = eval_int(
        r#"srand(2);
           my @r = sample(3, 1..10);
           my %src; @src{1..10} = ();
           my $bad = grep { !exists $src{$_} } @r;
           $bad ? 1 : 0"#,
    );
    assert_eq!(n, 0);
}

// ── Statement modifiers ─────────────────────────────────────────────────────

#[test]
fn for_modifier_appends_to_string() {
    assert_eq!(eval_string(r#"my $r = ""; $r .= $_ for 1..3; $r"#), "123");
}

#[test]
fn foreach_modifier_is_synonym_for_for() {
    assert_eq!(
        eval_string(r#"my $r = ""; $r .= "$_," foreach 1..3; $r"#),
        "1,2,3,"
    );
}

#[test]
fn while_modifier_increments_until_condition_fails() {
    assert_eq!(eval_int(r#"my $i = 0; $i++ while $i < 5; $i"#), 5);
}

#[test]
fn until_modifier_increments_until_condition_holds() {
    assert_eq!(eval_int(r#"my $i = 0; $i++ until $i >= 5; $i"#), 5);
}

#[test]
fn return_with_if_modifier_short_circuits() {
    assert_eq!(
        eval_string(
            r#"sub abs2 { my $x = shift; return -$x if $x < 0; $x }
               abs2(-5) . "/" . abs2(7)"#
        ),
        "5/7"
    );
}

// ── Ternary chain ───────────────────────────────────────────────────────────

#[test]
fn ternary_chain_picks_first_truthy_branch() {
    assert_eq!(
        eval_string(
            r#"my $x = 5;
               $x < 0 ? "neg" : $x == 0 ? "zero" : "pos""#
        ),
        "pos"
    );
}

// ── Dispatch table via hash of coderefs ─────────────────────────────────────

#[test]
fn dispatch_table_routes_to_correct_handler() {
    assert_eq!(
        eval_string(
            r#"my %disp = (
                 add => sub { $_[0] + $_[1] },
                 sub => sub { $_[0] - $_[1] },
                 mul => sub { $_[0] * $_[1] },
               );
               join(",", map { "$_:" . $disp{$_}->(10, 3) } qw(add sub mul))"#
        ),
        "add:13,sub:7,mul:30"
    );
}

// ── pairs() returns Pair objects (not arrayrefs) today ──────────────────────

#[test]
fn pairs_returns_pair_ref_kind_today() {
    // `pairs(a => 1, b => 2)` returns 2 elements with `ref` = "Pair".
    let out = eval_string(
        r#"my @r = pairs(a => 1, b => 2);
           scalar(@r) . ":" . ref($r[0])"#,
    );
    assert_eq!(out, "2:Pair");
}

#[test]
fn pair_object_does_not_array_deref_today() {
    // BUG-061: `@$pair` raises "Can't dereference non-reference as array".
    // Pair has its own accessor methods (`->key`, `->value`) but the
    // arrayref-style interface from List::Util's `pairs` is missing.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my @r = pairs(a => 1, b => 2); my @kv = @{$r[0]}; "@kv""#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

// ── Two-dot range in list context produces inclusive list ────────────────────

#[test]
fn two_dot_range_inclusive() {
    assert_eq!(eval_int(r#"my @a = (1..10); scalar @a"#), 10);
}

#[test]
fn three_dot_range_in_list_context_is_two_dot_synonym() {
    assert_eq!(eval_string(r#"my @a = (1...5); "@a""#), "1 2 3 4 5");
}

// ── Defined-or, logical-or, logical-and short-circuit ──────────────────────

#[test]
fn defined_or_returns_left_when_zero_present() {
    assert_eq!(eval_int(r#"my $x = 0; $x // 99"#), 0);
}

#[test]
fn logical_or_replaces_zero_with_default() {
    assert_eq!(eval_string(r#"my $x = 0; $x || "default""#), "default");
}

#[test]
fn logical_and_returns_right_when_left_is_truthy() {
    assert_eq!(eval_string(r#"my $x = "ok"; $x && "yes""#), "yes");
}

// ── Regex in conditional + capture binding ───────────────────────────────────

#[test]
fn regex_in_conditional_captures_dollar_one() {
    assert_eq!(
        eval_string(
            r#"my $x = "abc123";
               my $r = ($x =~ /(\d+)/) ? $1 : "none";
               $r"#
        ),
        "123"
    );
}

// ── --check / -c CLI flag accepts syntactically valid sources ───────────────
//
// We can't drive the CLI from inside `eval_string`, but we can pin the lib
// API equivalent: `parse(...)` should succeed when `--check` would.

#[test]
fn parse_accepts_simple_assignment() {
    assert!(
        stryke::parse("my $x = 1").is_ok(),
        "parse failed for trivially valid source"
    );
}

// ── `take` parses with bareword args (not just paren form) ──────────────────

#[test]
fn take_bareword_with_n_first_returns_empty_today() {
    // Same calling-convention quirk without parens.
    assert_eq!(eval_int(r#"my @r = take 3, 1..10; scalar @r"#), 0);
}

// ── group_by parse error today ──────────────────────────────────────────────

#[test]
fn group_by_with_sub_keyword_is_parse_error_today() {
    // BUG-062: `group_by(sub { ... }, list)` fails with "Expected Comma, got
    // Semicolon" on the call. Same root issue as partition's sub-form.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"my %g = group_by(sub { $_ % 2 }, 1..6);"#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

// ── Block-form callbacks for chunk_while, partition, etc. ───────────────────

#[test]
fn chunk_while_predicate_seeing_consecutive_pair() {
    // Predicate gets `$_[0]` (current) and `$_[1]` (next) — pin the API
    // shape to ensure it doesn't drift.
    assert_eq!(
        eval_string(
            r#"my @r = chunk_while(sub { $_[0] + 1 == $_[1] }, 1, 2, 4, 5, 7);
               scalar @r"#
        ),
        "3"
    );
}
