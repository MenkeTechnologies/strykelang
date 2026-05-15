//! Cross-builtin consistency pins. Quickly notice if the arg-order
//! convention or return-shape of related builtins drifts.

use crate::common::*;

// ── Array-builtin arg consistency: f(LIST) returns scalar ──────────

#[test]
fn sum_min_max_avg_all_take_list_and_return_scalar() {
    let code = r#"
        my @input = (3, 1, 4, 1, 5, 9, 2, 6);
        # All accept LIST and return scalar.
        (defined(sum(@input))
            && defined(min(@input))
            && defined(max(@input))
            && defined(avg(@input))) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── grep + map + sort + uniq + reverse: all return arrays ─────────

#[test]
fn map_grep_sort_uniq_reverse_all_return_arrays() {
    let code = r#"
        my @input = (3, 1, 4, 1, 5, 9, 2, 6);
        my @a = map  { _ * 2 } @input;
        my @b = grep { _ > 2 } @input;
        my @c = sort { _0 <=> _1 } @input;
        my @d = uniq @input;
        my @e = reverse @input;
        (len(@a) == 8
            && len(@b) > 0
            && len(@c) == 8
            && len(@d) > 0
            && len(@e) == 8) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String builtins: uc/lc/length all unary ────────────────────────

#[test]
fn uc_lc_length_are_unary() {
    let code = r#"
        my $s = "Hello";
        (uc($s) eq "HELLO"
            && lc($s) eq "hello"
            && length($s) == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── len() works on string, array, hash-keys, range ────────────────

#[test]
fn len_works_uniformly_across_containers() {
    let code = r#"
        my %h = (a => 1, b => 2);
        (len("hello") == 5
            && len([1, 2, 3]) == 3
            && len(keys %h) == 2
            && len((1:10)) == 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Sketch constructors: hll/topk/cms/t_digest/bloom_filter/roaring ─

#[test]
fn all_sketch_constructors_return_truthy_ref() {
    let code = r#"
        (ref(hll(14)) ne ""
            && ref(topk(3)) ne ""
            && ref(cms(2048, 5)) ne ""
            && ref(t_digest(100)) ne ""
            && ref(bloom_filter(100, 0.01)) ne ""
            && ref(roaring()) ne "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── _add convention: hll_add, td_add, topk_add, bloom_add ─────────

#[test]
fn all_sketch_add_builtins_take_handle_then_value() {
    let code = r#"
        my $h = hll(14);
        my $t = t_digest(100);
        my $tk = topk(3);
        my $b = bloom_filter(100, 0.01);
        my $r = roaring();
        hll_add($h, "x");
        td_add($t, 42);
        topk_add($tk, "x");
        bloom_add($b, "x");
        rb_add($r, 1);
        # No crashes; all sketches present.
        1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exists vs defined: hash member checks ──────────────────────────

#[test]
fn exists_and_defined_differ_for_undef_value() {
    let code = r#"
        my %h = (key => undef);
        (exists($h{key}) && !defined($h{key})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric vs string comparison ops ───────────────────────────────

#[test]
fn numeric_and_string_compare_ops_distinct() {
    let code = r#"
        # Numeric: 10 == 10, "10" == 10
        # String: "10" eq "10", "10" ne 10? Perl: "10" eq "10" yes; numifies "10" → 10
        (10 == 10
            && "10" == 10
            && "10" eq "10"
            && "abc" ne "abd") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── join: separator first, then LIST ───────────────────────────────

#[test]
fn join_separator_first_then_list() {
    let code = r#"
        my $r = join("-", "a", "b", "c");
        $r eq "a-b-c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── split: pattern first, then string ──────────────────────────────

#[test]
fn split_pattern_first_then_string() {
    let code = r#"
        my @r = split(/,/, "a,b,c");
        join("|", @r) eq "a|b|c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── push: array first, then values ─────────────────────────────────

#[test]
fn push_array_first_then_values() {
    let code = r#"
        my @a = (1, 2);
        push @a, 3, 4;
        join(",", @a) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unshift_array_first_then_values() {
    let code = r#"
        my @a = (3, 4);
        unshift @a, 1, 2;
        join(",", @a) eq "1,2,3,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── splice: array first, then offset, then length ─────────────────

#[test]
fn splice_array_first_offset_length() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        splice(@a, 1, 2);
        join(",", @a) eq "1,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── substr: string first, offset, optional length ─────────────────

#[test]
fn substr_string_first_then_offset_length() {
    let code = r#"
        substr("hello world", 6, 5) eq "world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── index: haystack first, needle, optional start ─────────────────

#[test]
fn index_haystack_first_then_needle() {
    let code = r#"
        index("hello world", "world") == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Map context: scalar @r vs list @r ─────────────────────────────

#[test]
fn map_returns_list_in_array_context() {
    let code = r#"
        my @r = map { _ * 2 } (1, 2, 3);
        len(@r) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ref() of all primary types ────────────────────────────────────

#[test]
fn ref_predicates_uniform_for_all_types() {
    let code = r#"
        my $sr = \1;
        my $ar = [1, 2, 3];
        my $hr = +{ a => 1 };
        my $cr = sub { 1 };
        (ref($sr) ne ""    # SCALAR or similar
            && ref($ar) =~ /ARRAY/
            && ref($hr) =~ /HASH/
            && ref($cr) =~ /CODE/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── join with sep "" reconstructs from chars ──────────────────────

#[test]
fn join_with_empty_sep_concatenates() {
    let code = r#"
        my $r = join("", "a", "b", "c");
        $r eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty input through any builtin doesn't crash ─────────────────

#[test]
fn empty_input_through_common_builtins_safe() {
    // Pin: sum on empty returns undef (not 0 like Perl's List::Util).
    // map/grep/sort/uniq/reverse on empty all return empty arrays.
    let code = r#"
        my @e;
        my $s_undef = !defined(sum(@e));
        my @m = map { _ * 2 } @e;
        my @g = grep { _ > 0 } @e;
        my @so = sort { _0 cmp _1 } @e;
        my @u = uniq(@e);
        my @r = reverse @e;
        ($s_undef && len(@m) == 0 && len(@g) == 0
            && len(@so) == 0 && len(@u) == 0 && len(@r) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── time() returns positive scalar ─────────────────────────────────

#[test]
fn time_returns_positive_scalar() {
    let code = r#"
        my $t = time();
        $t > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── now_ns returns much larger than time() ────────────────────────

#[test]
fn now_ns_much_larger_than_time() {
    let code = r#"
        (now_ns() / time()) > 1e8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── join + split round-trip ────────────────────────────────────────

#[test]
fn join_split_roundtrip() {
    let code = r#"
        my $orig = "alpha,beta,gamma,delta";
        my @parts = split /,/, $orig;
        my $back = join(",", @parts);
        $back eq $orig ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Concat + interpolation give same result ───────────────────────

#[test]
fn concat_and_interpolation_equivalent() {
    let code = r#"
        my $name = "alice";
        my $age = 30;
        my $a = "name=$name age=$age";
        my $b = "name=" . $name . " age=" . $age;
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
