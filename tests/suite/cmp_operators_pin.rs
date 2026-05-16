//! Comparison-operator pins. Numeric vs string ops, three-way <=>/cmp,
//! edge cases.

use crate::common::*;

// ── Numeric ==, !=, <, >, <=, >= ───────────────────────────────────

#[test]
fn numeric_equal() {
    let code = r#"
        (10 == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_not_equal() {
    let code = r#"
        (10 != 20) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_less_than() {
    let code = r#"
        (5 < 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_greater_than() {
    let code = r#"
        (10 > 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_le_inclusive() {
    let code = r#"
        ((5 <= 5) && (5 <= 10)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_ge_inclusive() {
    let code = r#"
        ((10 >= 10) && (10 >= 5)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String eq, ne, lt, gt, le, ge ─────────────────────────────────

#[test]
fn string_equal() {
    let code = r#"
        ("hello" eq "hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_not_equal() {
    let code = r#"
        ("hello" ne "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_less_than() {
    let code = r#"
        ("abc" lt "abd") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_greater_than() {
    let code = r#"
        ("zzz" gt "aaa") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_le_inclusive() {
    let code = r#"
        (("abc" le "abc") && ("abc" le "abd")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_ge_inclusive() {
    let code = r#"
        (("zzz" ge "zzz") && ("zzz" ge "yyy")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Three-way spaceship operator <=> ──────────────────────────────

#[test]
fn spaceship_less_returns_minus_one() {
    let code = r#"
        (3 <=> 5) == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn spaceship_equal_returns_zero() {
    let code = r#"
        (5 <=> 5) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn spaceship_greater_returns_one() {
    let code = r#"
        (7 <=> 5) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String three-way cmp ──────────────────────────────────────────

#[test]
fn cmp_less_returns_minus_one() {
    let code = r#"
        ("apple" cmp "banana") == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cmp_equal_returns_zero() {
    let code = r#"
        ("xxx" cmp "xxx") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cmp_greater_returns_one() {
    let code = r#"
        ("zzz" cmp "aaa") == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String vs numeric distinction ─────────────────────────────────

#[test]
fn string_compare_treats_10_lt_2_lexically() {
    let code = r#"
        ("10" lt "2") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn numeric_compare_treats_10_gt_2() {
    let code = r#"
        ("10" > "2") ? 1 : 0   # numeric coerce
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Float compare ─────────────────────────────────────────────────

#[test]
fn float_compare_within_epsilon() {
    let code = r#"
        my $a = 0.1 + 0.2;
        abs($a - 0.3) < 1e-15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn float_equality_imprecise() {
    let code = r#"
        # 0.1 + 0.2 != 0.3 in IEEE 754.
        my $a = 0.1 + 0.2;
        ($a != 0.3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Negative number compare ──────────────────────────────────────

#[test]
fn negative_compare() {
    let code = r#"
        ((-5 < -1) && (-5 < 0) && (-5 < 5)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zero_compare() {
    let code = r#"
        ((0 == 0) && (0 != 1) && !(0 < 0) && !(0 > 0)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sort using comparators ────────────────────────────────────────

#[test]
fn sort_with_spaceship_ascending() {
    let code = r#"
        my @r = sort { _0 <=> _1 } (3, 1, 4, 1, 5, 9);
        join(",", @r) eq "1,1,3,4,5,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn sort_with_cmp_alphabetical() {
    let code = r#"
        my @r = sort { _0 cmp _1 } ("delta", "alpha", "charlie", "bravo");
        join(",", @r) eq "alpha,bravo,charlie,delta" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Chained comparisons ──────────────────────────────────────────

#[test]
fn chained_compare_via_and() {
    let code = r#"
        my $x = 5;
        (($x > 0) && ($x < 10)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String compare edge: empty string ─────────────────────────────

#[test]
fn empty_string_compares() {
    let code = r#"
        (("" eq "") && ("" ne "x") && ("" lt "a")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric coercion for hybrid compare ──────────────────────────

#[test]
fn numeric_compare_coerces_string() {
    let code = r#"
        # "10" == 10 numerically.
        ("10" == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn string_compare_does_not_coerce() {
    let code = r#"
        # "10" ne 10? Both stringify, "10" eq "10".
        ("10" eq 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Spaceship + cmp inside sort with stable tiebreak ─────────────

#[test]
fn sort_by_multi_key_via_chain() {
    let code = r#"
        my @rows = (
            +{ dept => "qa",  age => 30 },
            +{ dept => "eng", age => 25 },
            +{ dept => "eng", age => 30 },
            +{ dept => "qa",  age => 25 },
        );
        my @sorted = sort {
            ($_0->{dept} cmp $_1->{dept})
                || ($_0->{age} <=> $_1->{age})
        } @rows;
        # eng:25, eng:30, qa:25, qa:30.
        ($sorted[0]->{dept} eq "eng" && $sorted[0]->{age} == 25
            && $sorted[3]->{dept} eq "qa" && $sorted[3]->{age} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric compare with explicit + 0 coerce ─────────────────────

#[test]
fn explicit_plus_zero_coerce_string_to_number() {
    let code = r#"
        my $n = "42" + 0;
        $n == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String compare in regex tokenization ─────────────────────────

#[test]
fn dedup_strings_via_string_eq() {
    let code = r#"
        my @input = ("a", "b", "a", "c", "b", "a");
        my %seen;
        my @unique;
        for my $x (@input) {
            next if exists $seen{$x};
            $seen{$x} = 1;
            push @unique, $x;
        }
        join(",", @unique) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── eq for hashref ref-identity ──────────────────────────────────

#[test]
fn aliased_hashref_compares_eq_via_string() {
    let code = r#"
        my $h = +{ x => 1 };
        my $alias = $h;
        # Both stringify identically.
        ("$h" eq "$alias") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Big values still compare correctly ───────────────────────────

#[test]
fn large_int_compare() {
    let code = r#"
        my $a = 9_999_999_999;
        my $b = 9_999_999_998;
        ($a > $b) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Unicode string compare via cmp ───────────────────────────────

#[test]
fn unicode_string_compare_by_codepoint() {
    let code = r#"
        # 'a' = 0x61, 'b' = 0x62, 'é' = 0xE9
        ("a" lt "b" && "b" lt "é") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
