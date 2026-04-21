//! Extra interpreter integration tests: numeric edges, aggregates, builtins, regex, and subs.

use crate::common::*;

#[test]
fn multiply_add_chain() {
    assert_eq!(eval_int("2 * 3 + 4"), 10);
    assert_eq!(eval_int("2 + 3 * 4"), 14);
}

#[test]
fn float_compare_eq() {
    assert_eq!(eval_int("3.0 == 3"), 1);
    assert_eq!(eval_int("3.1 == 3"), 0);
}

#[test]
fn string_empty_is_false_in_boolean() {
    assert_eq!(eval_int(r#""" ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#""0" ? 1 : 0"#), 0);
}

#[test]
fn negative_zero_add() {
    assert_eq!(eval_int("-0 + 5"), 5);
}

#[test]
fn list_three_scalars_partial_rhs() {
    assert_eq!(
        eval_int("my ($a, $b, $c) = (1, 2); (defined($a) ? 1 : 0) + (defined($b) ? 1 : 0) + (defined($c) ? 0 : 1)"),
        3
    );
}

#[test]
fn push_multiple_values_at_once() {
    assert_eq!(
        eval_string(r#"my @a = (1); push @a, 2, 3; join(",", @a)"#),
        "1,2,3"
    );
}

#[test]
fn pop_returns_last_element() {
    assert_eq!(eval_int("my @a = (7, 8, 9); pop @a"), 9);
}

#[test]
fn shift_returns_first_element() {
    assert_eq!(eval_int("my @a = (7, 8, 9); shift @a"), 7);
}

#[test]
fn array_copy_via_list() {
    assert_eq!(
        eval_string(r#"my @a = (1, 2); my @b = @a; join(",", @b)"#),
        "1,2"
    );
}

#[test]
fn hash_numeric_string_keys() {
    assert_eq!(eval_int(r#"my %h; $h{"1"} = 10; $h{1}"#), 10);
}

#[test]
fn delete_missing_key_undef() {
    assert_eq!(
        eval_int(r#"my %h = (a => 1); defined(delete $h{b}) ? 1 : 0"#),
        0
    );
}

#[test]
fn magic_constants_file_and_line() {
    assert_eq!(eval_int("__LINE__"), 1);
    assert_eq!(eval_string("__FILE__"), "-e");
}

#[test]
fn exists_delete_on_hash_reference_arrow() {
    assert_eq!(
        eval_int(r#"my $r = { a => 1, b => 2 }; (exists $r->{a}) + (exists $r->{z})"#),
        1
    );
    assert_eq!(
        eval_int(
            r#"my $r = { a => 1 }; my $v = delete $r->{a}; (exists $r->{a}) + (defined($v) ? 1 : 0)"#,
        ),
        1
    );
}

#[test]
fn keys_in_sorted_join() {
    assert_eq!(
        eval_string(r#"my %h = (z => 1, a => 2); join("", sort keys %h)"#),
        "az"
    );
}

#[test]
fn foreach_range_sum() {
    assert_eq!(
        eval_int("my $s = 0; foreach my $n (1..4) { $s = $s + $n; } $s"),
        10
    );
}

#[test]
fn c_for_multiple_init_style() {
    assert_eq!(
        eval_int(
            "my $s = 0; \
             for (my $i = 0; $i < 4; $i = $i + 1) { $s = $s + $i; } \
             $s",
        ),
        6
    );
}

#[test]
fn if_without_else_yields_undef_coerced() {
    assert_eq!(eval_int("my $x = 0; if (0) { $x = 1; } $x"), 0);
}

#[test]
fn elsif_reaches_third_branch() {
    assert_eq!(
        eval_int(
            "my $x = 3; \
             if ($x == 1) { 10 } elsif ($x == 2) { 20 } elsif ($x == 3) { 30 } else { 0 }",
        ),
        30
    );
}

#[test]
fn unless_else_branch() {
    assert_eq!(eval_int("my $x = 0; unless ($x) { 5 } else { 9 }"), 5);
}

#[test]
fn nested_loop_last_inner() {
    assert_eq!(
        eval_int(
            "my $t = 0; \
             foreach my $i (1..3) { \
                 foreach my $j (1..3) { \
                     $t = $t + 1; \
                     last if $j == 1; \
                 } \
             } \
             $t",
        ),
        3
    );
}

#[test]
fn while_next_increments() {
    assert_eq!(
        eval_int(
            "my $i = 0; my $s = 0; \
             while ($i < 6) { \
                 $i = $i + 1; \
                 next if $i % 2 == 0; \
                 $s = $s + $i; \
             } \
             $s",
        ),
        9
    );
}

#[test]
fn do_scalar_block_numeric() {
    assert_eq!(eval_int("do { 2 * 21 }"), 42);
}

#[test]
fn sprintf_decimal_negative() {
    assert_eq!(eval_string(r#"sprintf("%d", -3)"#), "-3");
}

#[test]
fn sprintf_minimum_width() {
    assert_eq!(eval_string(r#"sprintf("%5d", 7)"#), "    7");
}

#[test]
fn sprintf_percent_literal() {
    assert_eq!(eval_string(r#"sprintf("100%%")"#), "100%");
}

#[test]
fn chr_ord_roundtrip_ascii() {
    assert_eq!(eval_int(r#"ord(chr(33))"#), 33);
}

#[test]
fn abs_large_negative() {
    assert_eq!(eval_int("abs(-999)"), 999);
}

#[test]
fn hex_without_prefix() {
    assert_eq!(eval_int(r#"hex("10")"#), 16);
}

#[test]
fn oct_explicit_octal_string() {
    assert_eq!(eval_int(r#"oct("10")"#), 8);
}

#[test]
fn index_starts_after_offset() {
    assert_eq!(eval_int(r#"index("abab", "ab", 1)"#), 2);
}

#[test]
fn substr_omit_length_to_end() {
    assert_eq!(eval_string(r#"substr("abcdef", 2)"#), "cdef");
}

#[test]
fn ucfirst_lcfirst_preserves_rest() {
    assert_eq!(eval_string(r#"ucfirst("hello")"#), "Hello");
    assert_eq!(eval_string(r#"lcfirst("HELLO")"#), "hELLO");
}

#[test]
fn join_three_way() {
    assert_eq!(eval_string(r#"join(":", "a", "b", "c")"#), "a:b:c");
}

#[test]
fn reverse_empty_array_join() {
    assert_eq!(eval_string(r#"my @e = (); join(",", rev @e)"#), "");
}

#[test]
fn sort_reverse_numeric_via_block() {
    assert_eq!(
        eval_string(r#"join(",", sort { $b <=> $a } (1, 2, 3))"#),
        "3,2,1"
    );
}

#[test]
fn map_double_all() {
    assert_eq!(
        eval_string(r#"join(",", map { $_ * 2 } (1, 2, 3))"#),
        "2,4,6"
    );
}

#[test]
fn grep_even_numbers() {
    assert_eq!(
        eval_string(r#"join(",", grep { $_ % 2 == 0 } (1, 2, 3, 4))"#),
        "2,4"
    );
}

#[test]
fn regex_capture_after_match() {
    assert_eq!(eval_int(r#"my $s = "pin42tail"; $s =~ /(\d+)/; $1"#), 42);
}

#[test]
fn regex_alternation_first_branch() {
    assert_eq!(eval_int(r#""cat" =~ /cat|dog/ ? 1 : 0"#), 1);
}

#[test]
fn substitution_backreference() {
    assert_eq!(
        eval_string(r#"my $s = "ab"; $s =~ s/(.)(.)/$2$1/; $s"#),
        "ba"
    );
}

#[test]
fn match_with_anchors() {
    assert_eq!(eval_int(r#""hello" =~ /^hello$/ ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#""xhello" =~ /^hello$/ ? 1 : 0"#), 0);
}

#[test]
fn transliterate_lowercase_to_upper_range() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; $s =~ tr/a-z/A-Z/; $s"#),
        "ABC"
    );
}

#[test]
fn sub_adds_two_via_return() {
    assert_eq!(
        eval_int("sub sum2 { my $a = shift @_; my $b = shift @_; return $a + $b; } sum2(11, 31)"),
        42
    );
}

#[test]
fn sub_calls_sub() {
    assert_eq!(
        eval_int(
            "sub incr { my $x = shift @_; return $x + 1; } \
             sub twice { my $y = shift @_; return incr($y) + incr($y); } \
             twice(5)",
        ),
        12
    );
}

#[test]
fn lexical_closure_captures_outer_scalar() {
    assert_eq!(
        eval_int(
            "my $b = 2; \
             my $outer = fn { my $c = 3; return fn { return $b + $c; }; }; \
             my $inner = $outer->(); \
             $inner->()",
        ),
        5
    );
}

#[test]
fn eval_string_computed() {
    assert_eq!(eval_int(r#"eval("5 * 6")"#), 30);
}

#[test]
fn defined_array_element() {
    assert_eq!(eval_int(r#"my @a = (1); defined($a[0]) ? 1 : 0"#), 1);
}

#[test]
fn array_stringify_in_concat() {
    // Scalar `@a` is array length; string concat yields `"3"`.
    assert_eq!(eval_string(r#"my @a = (1, 2, 3); "" . @a"#), "3");
}

#[test]
fn negative_index_slice_end() {
    assert_eq!(
        eval_string(r#"my @a = (10, 20, 30, 40); join(",", @a[1, -1])"#),
        "20,40"
    );
}

#[test]
fn postfix_if_on_expression() {
    assert_eq!(eval_int("my $x = 0; $x = $x + 10 if 1; $x"), 10);
}

#[test]
fn postfix_unless_skips_when_condition_true() {
    assert_eq!(eval_int("my $x = 5; $x = 0 unless 1; $x"), 5);
}

#[test]
fn ternary_with_comparison() {
    assert_eq!(eval_int("my $a = 3; my $b = 4; $a > $b ? $a : $b"), 4);
}

#[test]
fn bitwise_or_combines_flags() {
    assert_eq!(eval_int("0x0C | 0x03"), 0x0F);
}

#[test]
fn bitwise_and_masks() {
    assert_eq!(eval_int("0b11110000 & 0b00001111"), 0);
}

#[test]
fn string_cmp_three_way_edge() {
    assert_eq!(eval_int(r#""" cmp "a""#), -1);
}

#[test]
fn spaceship_equal_operands() {
    assert_eq!(eval_int("5 <=> 5"), 0);
}

#[test]
fn file_test_directory_dot() {
    assert_eq!(eval_int("-d '.'"), 1);
}

#[test]
fn file_test_readable_dot() {
    assert_eq!(eval_int("-r '.'"), 1);
}

#[test]
fn ref_array_type() {
    assert_eq!(eval_string(r#"ref([1,2])"#), "ARRAY");
}

#[test]
fn array_ref_arrow_first() {
    assert_eq!(eval_int("my $r = [9, 8]; $r->[0]"), 9);
}

#[test]
fn hash_two_keys_fat_arrow_sum() {
    assert_eq!(eval_int("my %h = (aa => 3, bb => 4); $h{aa} + $h{bb}"), 7);
}

#[test]
fn package_var_two_statements() {
    assert_eq!(
        eval_int(
            "our $gv = 10; \
             our $gv2 = 20; \
             $gv + $gv2",
        ),
        30
    );
}

#[test]
fn local_changes_visible_inside_block() {
    assert_eq!(
        eval_int(
            "my $x = 1; \
             my $inner = do { local $x = 50; $x }; \
             $inner",
        ),
        50
    );
}
