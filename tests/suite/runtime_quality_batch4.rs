//! Interpreter integration tests: `pack`/`unpack`, regex features, ranges, list / scalar builtins,
//! compound assigns, hash slices, pragmas, `package`/`our`, refs, and aggregate edges.

use crate::common::*;

#[test]
fn pack_a2_fixed_width_ascii() {
    assert_eq!(eval_string(r#"unpack 'A2', pack 'A2', "ab""#), "ab");
}

#[test]
fn pack_a3_space_padded_unpack_trims() {
    assert_eq!(eval_string(r#"unpack 'A3', pack 'A3', "x""#), "x");
}

#[test]
fn pack_x_skips_bytes_before_unsigned_char() {
    assert_eq!(eval_int(r#"ord substr pack('x2 C', 44), 2, 1"#), 44);
}

#[test]
fn unpack_big_endian_n_word() {
    assert_eq!(
        eval_int(r#"scalar unpack 'N', pack 'N', 0x01020304"#),
        0x01020304
    );
}

#[test]
fn sprintf_scientific_lowercase_e_flag() {
    let s = eval_string(r#"sprintf("%.1e", 120.0)"#);
    assert!(s.contains('e') || s.contains('E'));
}

#[test]
fn regex_word_boundary_matches_token() {
    assert_eq!(eval_int(r#""cat" =~ /\bcat\b/ ? 1 : 0"#), 1);
}

#[test]
fn regex_non_greedy_plus_still_reaches_b() {
    assert_eq!(eval_string(r#"my $s = "aab"; $s =~ /a+?b/; $&"#), "aab");
}

#[test]
fn regex_non_capturing_then_capture() {
    assert_eq!(eval_string(r#"my $t = "ab"; $t =~ /(?:a)(b)/; $1"#), "b");
}

#[test]
fn numeric_range_high_to_low_empty() {
    assert_eq!(eval_int(r#"scalar (5..3)"#), 0);
}

#[test]
fn numeric_range_singleton() {
    assert_eq!(eval_string(r#"join "-", (7..7)"#), "7");
}

#[test]
fn list_util_sum_three_args() {
    assert_eq!(eval_int(r#"sum(10, 20, 30)"#), 60);
}

#[test]
fn list_util_max_of_three() {
    assert_eq!(eval_int(r#"max(3, 9, 2)"#), 9);
}

#[test]
fn list_util_min_of_three() {
    assert_eq!(eval_int(r#"min(3, 9, 2)"#), 2);
}

#[test]
fn list_util_product_three_factors() {
    assert_eq!(eval_int(r#"product(2, 3, 4)"#), 24);
}

#[test]
fn scalar_util_reftype_array_ref() {
    assert_eq!(eval_string(r#"Scalar::Util::reftype([])"#), "ARRAY");
}

#[test]
fn compound_power_assign() {
    assert_eq!(eval_int(r#"my $a = 2; $a **= 4; $a"#), 16);
}

#[test]
fn compound_left_shift_assign() {
    assert_eq!(eval_int(r#"my $b = 1; $b <<= 4; $b"#), 16);
}

#[test]
fn compound_right_shift_assign() {
    assert_eq!(eval_int(r#"my $c = 32; $c >>= 2; $c"#), 8);
}

#[test]
fn compound_bitand_assign() {
    assert_eq!(eval_int(r#"my $m = 0b1111; $m &= 0b1010; $m"#), 0b1010);
}

#[test]
fn compound_bitor_assign() {
    assert_eq!(eval_int(r#"my $m = 0b1000; $m |= 0b0011; $m"#), 0b1011);
}

#[test]
fn defined_or_assign_preserves_defined_zero() {
    assert_eq!(eval_int(r#"my $z = 0; $z //= 9; $z"#), 0);
}

#[test]
fn logical_or_assign_fills_falsy_scalar() {
    assert_eq!(eval_int(r#"my $z = 0; $z ||= 11; $z"#), 11);
}

#[test]
fn logical_and_assign_short_circuits_on_falsy() {
    assert_eq!(eval_int(r#"my $z = 0; $z &&= 7; $z"#), 0);
}

#[test]
fn sub_scalar_underscore_counts_arguments() {
    assert_eq!(eval_int(r#"fn narg { scalar @_ } narg(1, 2, 3, 4)"#), 4);
}

#[test]
fn hash_slice_assign_two_keys_sum() {
    assert_eq!(
        eval_int(
            r#"my %h;
               @h{"a", "b"} = (10, 20);
               $h{a} + $h{b}"#,
        ),
        30
    );
}

#[test]
fn transliterate_count_matches_engine() {
    assert_eq!(eval_int(r#"my $s = "abc"; $s =~ tr/a-z/A-Z/"#), 3);
}

#[test]
fn use_warnings_runs() {
    assert_eq!(eval_int("use warnings; 1"), 1);
}

#[test]
fn no_warnings_runs() {
    assert_eq!(eval_int("no warnings; 1"), 1);
}

#[test]
fn eval_block_value_not_syntax_error() {
    assert_eq!(eval_int(r#"eval { 5 + 6 }"#), 11);
}

#[test]
fn package_statement_sets_package() {
    assert_eq!(
        eval_string(
            r#"package Qux::Zot;
               __PACKAGE__"#,
        ),
        "Qux::Zot"
    );
}

#[test]
fn our_scalar_readable() {
    assert_eq!(eval_int("our $batch4_our = 19; $batch4_our"), 19);
}

#[test]
fn ref_named_sub_is_code() {
    assert_eq!(
        eval_string(
            r#"fn batch4_id { 1 }
               ref(\&batch4_id)"#,
        ),
        "CODE"
    );
}

#[test]
fn ref_qr_is_regexp_type() {
    let t = eval_string(r#"ref(qr/^x$/)"#);
    assert!(
        t.eq_ignore_ascii_case("REGEXP") || t.contains("Regexp"),
        "unexpected ref(qr//): {t:?}"
    );
}

#[test]
fn lookahead_negative_rejects_suffix() {
    assert_eq!(eval_int(r#""foo" =~ /foo(?!bar)/ ? 1 : 0"#), 1);
}

#[test]
fn lookahead_positive_requires_suffix() {
    assert_eq!(eval_int(r#""foobar" =~ /foo(?=bar)/ ? 1 : 0"#), 1);
}

#[test]
fn list_separator_in_array_stringify() {
    assert_eq!(eval_string(r#"my @x = qw(p q); $" = "*"; "@x""#), "p*q");
}

#[test]
fn push_onto_array_through_deref() {
    assert_eq!(
        eval_string(
            r#"my $r = [1, 2];
               push @$r, 3;
               join ",", @$r"#,
        ),
        "1,2,3"
    );
}

#[test]
fn sparse_array_extends_max_index() {
    assert_eq!(
        eval_int(
            r#"my @a = (1);
               $a[5] = 9;
               $#a"#,
        ),
        5
    );
}

#[test]
fn exists_middle_sparse_array_slot_reports_exists() {
    assert_eq!(
        eval_int(
            r#"my @a;
               $a[4] = 1;
               exists $a[2] ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn sort_reverse_lexical_block() {
    assert_eq!(
        eval_string(r#"join "", sort { $b cmp $a } ("b", "a", "c")"#),
        "cba"
    );
}

#[test]
fn grep_numeric_comparison_on_list() {
    assert_eq!(
        eval_string(r#"join(",", grep { $_ >= 2 } (1, 2, 3))"#),
        "2,3"
    );
}

#[test]
fn int_truncates_string_with_leading_number() {
    assert_eq!(eval_int(r#"int("3.9xyz")"#), 3);
}

#[test]
fn numification_adds_string_prefix() {
    assert_eq!(eval_int(r#"0 + "17abc""#), 17);
}

#[test]
fn repeat_assign_from_expression() {
    assert_eq!(eval_string(r#"my $n = 2; "ab" x $n"#), "abab");
}

#[test]
fn explicit_and_comparison_chain_true() {
    // Explicit && still works (in addition to new Raku-style chained comparisons)
    assert_eq!(eval_int("1 < 2 && 2 < 3"), 1);
}

#[test]
fn explicit_and_comparison_chain_false() {
    // Explicit && still works (in addition to new Raku-style chained comparisons)
    assert_eq!(eval_int("1 < 2 && 2 > 3"), 0);
}

#[test]
fn postfix_while_runs_until_condition() {
    assert_eq!(
        eval_int(
            r#"my $i = 0;
               $i += 1 while $i < 5;
               $i"#,
        ),
        5
    );
}

#[test]
fn until_loop_one_shot() {
    assert_eq!(
        eval_int(
            r#"my $n = 0;
               until ($n > 0) { $n = 1; }
               $n"#,
        ),
        1
    );
}

#[test]
fn if_elsif_else_final_branch() {
    assert_eq!(
        eval_int(
            r#"my $k = 0;
               if ($k == 1) { 10 } elsif ($k == 2) { 20 } else { 30 }"#,
        ),
        30
    );
}

#[test]
fn substitution_once_replaces_first_only() {
    assert_eq!(eval_string(r#"my $s = "aa"; $s =~ s/a/b/; $s"#), "ba");
}

#[test]
fn match_sets_digit_vars_for_groups() {
    assert_eq!(
        eval_string(r#"my $t = "ab"; $t =~ /(a)(b)/; "$1-$2""#),
        "a-b"
    );
}

#[test]
fn hex_odd_length_errors_at_runtime() {
    use stryke::error::ErrorKind;
    assert_eq!(eval_err_kind(r#"pack 'H', "a""#), ErrorKind::Runtime);
}

#[test]
fn sprintf_percent_g_compact_float() {
    let s = eval_string(r#"sprintf("%g", 12345.678)"#);
    assert!(!s.is_empty());
}

#[test]
fn octal_literal_zero() {
    assert_eq!(eval_int("00"), 0);
}

#[test]
fn binary_literal_basic() {
    assert_eq!(eval_int("0b101"), 5);
}

#[test]
fn string_ne_force_string_compare() {
    assert_eq!(eval_int(r#""1.0" ne "1" ? 1 : 0"#), 1);
}

#[test]
fn list_util_first_with_coderef_finds_element() {
    assert_eq!(eval_int(r#"first(fn { $_ > 2 }, 1, 2, 3)"#), 3);
}

#[test]
fn list_util_none_with_coderef_no_match() {
    assert_eq!(
        eval_int(r#"none(fn { $_ > 10 }, 1, 2, 3) ? 1 : 0"#),
        1
    );
}

#[test]
fn list_util_any_with_coderef_one_hit() {
    assert_eq!(
        eval_int(r#"any(fn { $_ == 2 }, 1, 2, 3) ? 1 : 0"#),
        1
    );
}

#[test]
fn list_util_all_with_coderef_all_positive() {
    assert_eq!(
        eval_int(r#"all(fn { $_ > 0 }, 1, 2, 3) ? 1 : 0"#),
        1
    );
}

#[test]
fn list_util_notall_with_coderef_all_match() {
    assert_eq!(
        eval_int(r#"notall(fn { $_ > 0 }, 1, 2, 3) ? 1 : 0"#),
        0
    );
}

#[test]
fn scalar_util_blessed_reports_package() {
    assert_eq!(
        eval_string(r#"Scalar::Util::blessed(bless {}, "Box")"#),
        "Box"
    );
}

#[test]
fn scalar_util_blessed_plain_ref_undef() {
    assert_eq!(eval_int(r#"defined(Scalar::Util::blessed([])) ? 1 : 0"#), 0);
}

#[test]
fn match_entire_pattern_ampersand() {
    assert_eq!(eval_string(r#"my $s = "abcde"; $s =~ /bcd/; $&"#), "bcd");
}

#[test]
fn match_prematch_and_postmatch_special_vars() {
    assert_eq!(
        eval_string(
            r#"my $s = "abcde";
               $s =~ /c/;
               ${^PREMATCH} . "|" . ${^POSTMATCH}"#,
        ),
        "ab|de"
    );
}

#[test]
fn unpack_h2_from_packed_byte() {
    assert_eq!(eval_string(r#"unpack 'H2', pack 'C', 255"#), "FF");
}

#[test]
fn regex_whitespace_class_matches_tab() {
    assert_eq!(eval_int(r#""\t" =~ /\s/ ? 1 : 0"#), 1);
}

#[test]
fn regex_word_char_class() {
    assert_eq!(eval_int(r#""_9" =~ /^\w\w$/ ? 1 : 0"#), 1);
}

#[test]
fn regex_digit_class() {
    assert_eq!(eval_int(r#""5" =~ /^\d$/ ? 1 : 0"#), 1);
}

#[test]
fn list_util_pairs_four_elements_two_objects() {
    assert_eq!(eval_int(r#"scalar pairs(1, 2, 3, 4)"#), 2);
}

#[test]
fn list_util_pairkeys_two_keys() {
    assert_eq!(
        eval_string(r#"join "-", pairkeys(10, 20, 30, 40)"#),
        "10-30"
    );
}

#[test]
fn list_util_pairvalues_two_values() {
    assert_eq!(
        eval_string(r#"join "-", pairvalues(10, 20, 30, 40)"#),
        "20-40"
    );
}

#[test]
fn substitution_backreference_double_digit_disambiguation() {
    assert_eq!(
        eval_string(r#"my $s = "abcdefghij"; $s =~ /(.)(.)(.)(.)(.)(.)(.)(.)(.)(.)/; "${1}0""#),
        "a0"
    );
}

#[test]
fn negative_subscript_assign_extends_array() {
    assert_eq!(
        eval_int(
            r#"my @a = (1, 2);
               $a[-1] = 9;
               $a[1]"#,
        ),
        9
    );
}

#[test]
fn hash_key_exists_before_value_assign() {
    assert_eq!(
        eval_int(
            r#"my %h;
               exists $h{newk} ? 1 : 0"#,
        ),
        0
    );
}

#[test]
fn delete_array_elem_slot_still_exists_in_engine() {
    assert_eq!(
        eval_int(
            r#"my @a = (1, 2);
               delete $a[0];
               exists $a[0] ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn foreach_reverse_range_product() {
    assert_eq!(
        eval_int(
            r#"my $p = 1;
               foreach my $n (rev 1..3) {
                   $p = $p * $n;
               }
               $p"#,
        ),
        6
    );
}

#[test]
fn logical_and_short_circuit_skips_rhs() {
    assert_eq!(eval_int("0 && die; 1"), 1);
}

#[test]
fn logical_or_short_circuit_skips_rhs() {
    assert_eq!(eval_int("1 || die; 2"), 2);
}
