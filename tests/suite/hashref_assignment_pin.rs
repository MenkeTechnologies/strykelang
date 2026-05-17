//! Hashref mutation + deep-clone pins beyond hashref_deep_pin.rs.

use crate::common::*;

// ── Direct mutation through arrow ──────────────────────────────────

#[test]
fn write_via_arrow_visible_through_alias() {
    let code = r#"
        my $h = +{ a => 1 };
        my $alias = $h;
        $alias->{a} = 99;
        $h->{a} == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn write_via_alias_does_not_clone() {
    let code = r#"
        my $orig = +{ name => "alice", age => 30 };
        my $copy = $orig;     # alias, not deep copy
        $copy->{age} = 31;
        $orig->{age} == 31 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Deep clone via JSON round-trip ────────────────────────────────

#[test]
fn deep_clone_via_json_isolates_writes() {
    let code = r#"
        my $orig = +{ a => +{ b => 1 } };
        my $clone = from_json(to_json($orig));
        $clone->{a}->{b} = 99;
        $orig->{a}->{b} == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Merge two hashrefs ─────────────────────────────────────────────

#[test]
fn merge_two_hashrefs_via_explicit_loop() {
    let code = r#"
        my $a = +{ x => 1, y => 2 };
        my $b = +{ y => 20, z => 3 };   # y collides
        my %merged = %$a;
        for my $k (keys %$b) {
            $merged{$k} = $b->{$k};      # b overrides a
        }
        ($merged{x} == 1 && $merged{y} == 20 && $merged{z} == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn merge_via_double_deref() {
    let code = r#"
        my $a = +{ x => 1, y => 2 };
        my $b = +{ z => 3 };
        my %merged = (%$a, %$b);
        (len(keys %merged) == 3 && $merged{z} == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Builder pattern: incrementally fill hashref ───────────────────

#[test]
fn builder_pattern_increments_via_arrow_assign() {
    let code = r#"
        my $h = +{};
        $h->{a} = 1;
        $h->{b} = 2;
        $h->{c} = 3;
        (len(keys %$h) == 3 && $h->{b} == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Conditional default via // ─────────────────────────────────────

#[test]
fn defined_or_for_default_value() {
    let code = r#"
        my $h = +{ a => 1 };
        my $v = $h->{missing} // 99;
        $v == 99 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_or_returns_present_zero_unchanged() {
    let code = r#"
        my $h = +{ counter => 0 };
        my $v = $h->{counter} // 99;
        $v == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Conditional set: increment via //= or ternary ─────────────────

#[test]
fn increment_or_init_via_defined_or_assign() {
    let code = r#"
        my %h;
        for my $k ("a", "b", "a", "c", "a") {
            $h{$k} = ($h{$k} // 0) + 1;
        }
        ($h{a} == 3 && $h{b} == 1 && $h{c} == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Group-by pattern via push ─────────────────────────────────────

#[test]
fn group_by_via_explicit_init() {
    let code = r#"
        my @items = (
            +{ k => "a", v => 1 },
            +{ k => "b", v => 2 },
            +{ k => "a", v => 3 },
            +{ k => "b", v => 4 },
        );
        my %groups;
        for my $it (@items) {
            $groups{$it->{k}} = [] unless exists $groups{$it->{k}};
            push @{$groups{$it->{k}}}, $it->{v};
        }
        (len(@{$groups{a}}) == 2 && len(@{$groups{b}}) == 2
            && $groups{a}->[1] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hashref keys iteration via map ─────────────────────────────────

#[test]
fn map_over_hashref_keys() {
    let code = r#"
        my $h = +{ alpha => 1, beta => 2, gamma => 3 };
        my @doubled = sort map { $h->{$_} * 2 } keys %$h;
        join(",", @doubled) eq "2,4,6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map-style transform: build new hashref from old ───────────────

#[test]
fn transform_hashref_into_new_hashref() {
    let code = r#"
        my $orig = +{ alice => 80, bob => 90, carol => 70 };
        my %new;
        for my $k (keys %$orig) {
            $new{$k} = $orig->{$k} + 10;
        }
        ($new{alice} == 90 && $new{bob} == 100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Delete + check ─────────────────────────────────────────────────

#[test]
fn delete_returns_removed_value() {
    let code = r#"
        my $h = +{ a => "alpha", b => "beta" };
        my $removed = delete $h->{a};
        ($removed eq "alpha" && !exists $h->{a}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Clone-and-mutate idiom ─────────────────────────────────────────

#[test]
fn clone_via_double_deref_then_arrayref() {
    let code = r#"
        my $orig = +{ a => 1, b => 2 };
        my %copy = %$orig;
        my $cloned = \%copy;
        $cloned->{a} = 99;
        $orig->{a} == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested write requires explicit init ────────────────────────────

#[test]
fn nested_write_after_explicit_init() {
    // BUG-216: no auto-vivification.
    let code = r#"
        my $h = +{};
        $h->{level1}             = +{};
        $h->{level1}->{level2}   = +{};
        $h->{level1}->{level2}->{leaf} = "deep";
        $h->{level1}->{level2}->{leaf} eq "deep" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Conditional create via exists ─────────────────────────────────

#[test]
fn conditional_create_idiom_with_exists() {
    let code = r#"
        my $h = +{};
        for my $k ("a", "b", "a") {
            $h->{$k} = +{ count => 0 } unless exists $h->{$k};
            $h->{$k}->{count} = $h->{$k}->{count} + 1;
        }
        ($h->{a}->{count} == 2 && $h->{b}->{count} == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hashref equality semantics (ref-identity not deep) ────────────

#[test]
fn two_distinct_hashrefs_stringify_identically() {
    // Stryke surface: ALL hashrefs stringify to the placeholder
    // "HASH(0x...)" literally — no distinct hex address. So two
    // distinct refs compare eq via string interpolation. Pin the
    // observed behavior; deep equality must be via codec roundtrip.
    let code = r#"
        my $a = +{ x => 1 };
        my $b = +{ y => 2 };
        ("$a" eq "$b") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn aliased_hashref_compares_eq_via_string_id() {
    let code = r#"
        my $h = +{ x => 1 };
        my $alias = $h;
        "$h" eq "$alias" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hashref into JSON keeps keys ──────────────────────────────────

#[test]
fn hashref_to_json_includes_all_keys() {
    let code = r#"
        my $h = +{ a => 1, b => 2, c => 3 };
        my $j = to_json($h);
        # Keys all appear in JSON.
        (index($j, "\"a\"") >= 0
            && index($j, "\"b\"") >= 0
            && index($j, "\"c\"") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Inline construction with computed keys ────────────────────────

#[test]
fn inline_hashref_with_computed_key() {
    let code = r#"
        my $prefix = "user_";
        my $h = +{ ("${prefix}id") => 42, ("${prefix}name") => "alice" };
        ($h->{user_id} == 42 && $h->{user_name} eq "alice") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exists returns 0/1 ────────────────────────────────────────────

#[test]
fn exists_returns_boolean() {
    let code = r#"
        my $h = +{ a => 1 };
        my $r1 = exists $h->{a};
        my $r2 = exists $h->{nope};
        # Truthy/falsy.
        ($r1 && !$r2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Update via map produces new hash ──────────────────────────────

#[test]
fn produce_uppercased_keys() {
    let code = r#"
        my $h = +{ alpha => 1, beta => 2 };
        my %new;
        for my $k (keys %$h) {
            $new{uc($k)} = $h->{$k};
        }
        ($new{ALPHA} == 1 && $new{BETA} == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reverse a hash (value -> key) ─────────────────────────────────

#[test]
fn reverse_hash_via_value_key_swap() {
    let code = r#"
        my $h = +{ a => 1, b => 2, c => 3 };
        my %rev;
        for my $k (keys %$h) {
            $rev{$h->{$k}} = $k;
        }
        ($rev{1} eq "a" && $rev{3} eq "c") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Count keys via len ────────────────────────────────────────────

#[test]
fn count_keys_via_len() {
    let code = r#"
        my $h = +{ a => 1, b => 2, c => 3, d => 4, e => 5 };
        len(keys %$h) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Compound-assign on hash arrow-deref leaves the new value on stack ──
//
// Regression: `$h->{k} OP= v` compiled `Op::SetArrowHash` (no-keep), so
// the statement-level `Pop` after the expression then popped a slot
// from the CALLER'S frame, silently corrupting any expression that called
// the same sub multiple times. The fix: emit `Op::SetArrowHashKeep` so
// the new value lives on the stack as the expression value and the
// statement-level Pop only discards that single value.
//
// Shape-1: same sub called twice in one expression — Add must see both
// return values, not one + caller-stack-junk.

#[test]
fn arrow_hash_compound_assign_no_caller_stack_corruption_add() {
    let code = r#"
        fn FOO::dec($x) { $x->{n} -= 1; 1 }
        my $h = +{ n => 10 };
        FOO::dec($h) + FOO::dec($h) + FOO::dec($h)
    "#;
    assert_eq!(eval_int(code), 3);
}

// Shape-2: accumulator in a single-statement for-body — every iteration's
// fn return value must contribute to the sum.
#[test]
fn arrow_hash_compound_assign_no_caller_stack_corruption_for_loop() {
    let code = r#"
        fn FOO::dec($x) { $x->{n} -= 1; 1 }
        my $h = +{ n => 10 };
        my $tot = 0;
        for (1:5) { $tot += FOO::dec($h) }
        $tot
    "#;
    assert_eq!(eval_int(code), 5);
}

// Shape-3: each of the four common compound ops on hash arrow-deref —
// all must leave their new value on the stack.
#[test]
fn arrow_hash_compound_assign_keep_new_value_minus_eq() {
    let code = r#"
        my $h = +{ n => 10 };
        $h->{n} -= 3
    "#;
    assert_eq!(eval_int(code), 7);
}

#[test]
fn arrow_hash_compound_assign_keep_new_value_plus_eq() {
    let code = r#"
        my $h = +{ n => 10 };
        $h->{n} += 5
    "#;
    assert_eq!(eval_int(code), 15);
}

#[test]
fn arrow_hash_compound_assign_keep_new_value_mul_eq() {
    let code = r#"
        my $h = +{ n => 4 };
        $h->{n} *= 6
    "#;
    assert_eq!(eval_int(code), 24);
}
