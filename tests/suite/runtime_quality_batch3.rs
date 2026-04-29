//! More interpreter integration tests: `sleep`/`times`, JSON helpers, path helpers, regex flags,
//! `wantarray` list path, control-flow, refs/subs, `sprintf`, and `pack` edges.

use crate::common::*;

use stryke::error::ErrorKind;

#[test]
fn sleep_zero_returns_quickly() {
    assert_eq!(eval_int("sleep 0"), 0);
}

#[test]
fn times_returns_four_float_fields() {
    assert_eq!(eval_int(r#"scalar times()"#), 4);
}

#[test]
fn json_encode_integer_is_ascii_digits() {
    assert_eq!(eval_string(r#"json_encode(42)"#), "42");
}

#[test]
fn json_decode_integer_roundtrip() {
    assert_eq!(eval_int(r#"json_decode("99")"#), 99);
}

#[test]
fn json_encode_decode_string_roundtrip() {
    assert_eq!(
        eval_string(r#"json_decode(json_encode("stryke"))"#),
        "stryke"
    );
}

#[test]
fn json_encode_decode_true_false() {
    assert_eq!(eval_int(r#"json_decode("true")"#), 1);
    assert_eq!(eval_int(r#"json_decode("false")"#), 0);
}

#[test]
fn json_encode_decode_null_is_undef() {
    assert_eq!(eval_int(r#"defined(json_decode("null")) ? 1 : 0"#), 0);
}

#[test]
fn json_encode_array_decodes_to_list_count() {
    assert_eq!(eval_int(r#"len(@{[json_decode("[1,2,3]")]})"#), 3);
}

#[test]
fn json_decode_invalid_is_runtime_error() {
    assert_eq!(eval_err_kind(r#"json_decode("{")"#), ErrorKind::Runtime);
}

#[test]
fn canonpath_dot_is_dot_or_normalized() {
    let s = eval_string(r#"canonpath(".")"#);
    assert!(!s.is_empty());
}

#[test]
fn gethostname_non_empty() {
    assert!(!eval_string(r#"gethostname()"#).is_empty());
}

#[cfg(unix)]
#[test]
fn getppid_positive_on_unix() {
    assert!(eval_int("getppid()") > 0);
}

#[test]
fn regex_case_insensitive_flag() {
    assert_eq!(eval_int(r#""ABC" =~ /b/i ? 1 : 0"#), 1);
}

#[test]
fn substitution_case_insensitive_flag_lowercases_replacement() {
    assert_eq!(eval_string(r#"my $s = "AaA"; $s =~ s/a/x/gi; $s"#), "xxx");
}

#[test]
fn join_separate_scalars_not_from_sub_return_list() {
    assert_eq!(eval_string(r#"join "-", 1, 2"#), "1-2");
}

#[test]
fn wantarray_scalar_branch_in_sub() {
    assert_eq!(
        eval_int(
            r#"fn pair { wantarray ? (1, 2) : 9 }
               pair()"#,
        ),
        9
    );
}

#[test]
fn nested_ternary_right_associative() {
    assert_eq!(eval_int("0 ? 1 : 1 ? 2 : 3"), 2);
}

#[test]
fn unless_block_runs_on_false() {
    assert_eq!(eval_int("my $x = 0; unless ($x) { 44 } else { 0 }"), 44);
}

#[test]
fn logical_not_double_bang() {
    assert_eq!(eval_int("!!0"), 0);
    assert_eq!(eval_int("!!7"), 1);
}

#[test]
fn while_next_if_skips_one_value() {
    assert_eq!(
        eval_int(
            r#"my $i = 0;
               my $s = 0;
               while ($i < 4) {
                   $i = $i + 1;
                   next if $i == 2;
                   $s = $s + $i;
               }
               $s"#,
        ),
        8
    );
}

#[test]
fn foreach_keys_hash_accumulates_values() {
    assert_eq!(
        eval_int(
            r#"my %h = (a => 2, b => 3);
               my $t = 0;
               foreach my $k (keys %h) {
                   $t = $t + $h{$k};
               }
               $t"#,
        ),
        5
    );
}

#[test]
fn elsif_chain_second_branch() {
    assert_eq!(
        eval_int(
            r#"my $n = 2;
               if ($n == 0) { 0 } elsif ($n == 2) { 20 } else { 99 }"#,
        ),
        20
    );
}

#[test]
fn postdecrement_returns_prior_value() {
    assert_eq!(eval_int(r#"my $n = 8; $n--"#), 8);
}

#[test]
fn predecrement_mutates_before_use() {
    assert_eq!(eval_int(r#"my $n = 8; --$n"#), 7);
}

#[test]
fn compound_div_assign_scalar() {
    assert_eq!(eval_int(r#"my $x = 100; $x /= 4; $x"#), 25);
}

#[test]
fn compound_mod_assign_scalar() {
    assert_eq!(eval_int(r#"my $x = 17; $x %= 5; $x"#), 2);
}

#[test]
fn negative_array_index_read() {
    assert_eq!(eval_int(r#"my @a = (10, 20, 30); $a[-2]"#), 20);
}

#[test]
fn array_single_subscript_second_elem() {
    assert_eq!(eval_int(r#"my @a = (5, 6, 7); $a[1]"#), 6);
}

#[test]
fn anon_sub_invoked_with_arrow() {
    assert_eq!(eval_int(r#"(fn ($x) { $x * 3 })->(4)"#), 12);
}

#[test]
fn sub_positional_parameter_via_subscript_zero() {
    assert_eq!(
        eval_int(r#"fn pick_first { $_[0] } pick_first(100, 200)"#),
        100
    );
}

#[test]
fn bless_anon_array_ref_type() {
    assert_eq!(eval_string(r#"my $o = bless [], "Row"; ref $o"#), "Row");
}

#[test]
fn ref_type_scalar_reference() {
    assert_eq!(eval_string(r#"my $x = 1; ref(\$x)"#), "SCALAR");
}

#[test]
fn prototype_core_length_empty_in_engine() {
    assert_eq!(eval_string(r#"prototype "CORE::length""#), "");
}

#[test]
fn fileparse_three_tuple_middle_is_dir_without_trailing_slash() {
    assert_eq!(eval_string(r#"(fileparse("/tmp/x/y.pl"))[1]"#), "/tmp/x");
}

#[test]
fn basename_dirname_concat_restores_path() {
    assert_eq!(
        eval_string(
            r#"my $p = "/a/b/c.txt";
               dirname($p) . "/" . basename($p)"#,
        ),
        "/a/b/c.txt"
    );
}

#[test]
fn sprintf_percent_u_unsigned() {
    assert_eq!(eval_string(r#"sprintf("%u", 42)"#), "42");
}

#[test]
fn sprintf_left_pad_zeros() {
    assert_eq!(eval_string(r#"sprintf("%05d", 7)"#), "00007");
}

#[test]
fn hex_builtin_two_char_string() {
    assert_eq!(eval_int(r#"hex("2a")"#), 42);
}

#[test]
fn oct_binary_prefix_in_oct_builtin() {
    assert_eq!(eval_int(r#"oct("0b1010")"#), 10);
}

#[test]
fn match_on_lexical_scalar() {
    assert_eq!(
        eval_int(
            r#"my $t = "needle";
               $t =~ /eed/ ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn substitution_e_modifier_expression() {
    assert_eq!(
        eval_string(r#"my $s = "2"; $s =~ s/(\d)/$1 * 3/e; $s"#),
        "6"
    );
}

#[test]
fn pack_z_nul_terminator_length() {
    assert_eq!(eval_int(r#"length pack 'Z', "ab""#), 3);
}

#[test]
fn qw_word_list_scalar_count() {
    assert_eq!(eval_int(r#"scalar qw(one two three)"#), 3);
}

#[test]
fn qq_brace_delimiter() {
    assert_eq!(eval_string(r#"qq{curly}"#), "curly");
}

#[test]
fn scalar_hash_fill_string_has_slash() {
    let s = eval_string(r#"my %h = (a => 1, b => 2); scalar %h"#);
    assert!(s.contains('/'));
}

#[test]
fn list_range_three_elements_joined() {
    assert_eq!(eval_string(r#"join "-", (10..12)"#), "10-11-12");
}

#[test]
fn repeat_count_from_variable() {
    assert_eq!(eval_string(r#"my $n = 3; "z" x $n"#), "zzz");
}

#[test]
fn defined_array_index_after_pop_stays_defined_slot() {
    assert_eq!(
        eval_int(
            r#"my @a = (1);
               pop @a;
               defined $a[0] ? 1 : 0"#,
        ),
        0
    );
}

#[test]
fn push_onto_empty_then_shift() {
    assert_eq!(
        eval_int(
            r#"my @a = ();
               push @a, 5;
               shift @a"#,
        ),
        5
    );
}

#[test]
fn hash_delete_returns_value() {
    assert_eq!(eval_int(r#"my %h = (k => 33); delete $h{k}"#), 33);
}

#[test]
fn array_element_exists_after_growth() {
    assert_eq!(
        eval_int(
            r#"my @a;
               $a[2] = 9;
               exists $a[2] ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn do_block_last_statement_value() {
    assert_eq!(eval_int(r#"do { my $x = 3; $x + 4 }"#), 7);
}

#[test]
fn inner_lexical_my_does_not_change_outer_scalar() {
    assert_eq!(
        eval_int(
            r#"my $x = 1;
               do { my $x = 50; $x };
               $x"#,
        ),
        1
    );
}

#[test]
fn string_gt_lexical() {
    assert_eq!(eval_int(r#""z" gt "a" ? 1 : 0"#), 1);
}

#[test]
fn string_lt_lexical() {
    assert_eq!(eval_int(r#""a" lt "m" ? 1 : 0"#), 1);
}

#[test]
fn bitwise_shift_left_on_negative() {
    assert_eq!(eval_int("-1 << 1"), -2);
}

#[test]
fn sprintf_percent_x_lowercase_hex() {
    assert_eq!(eval_string(r#"sprintf("%x", 10)"#), "a");
}

#[test]
fn sprintf_percent_uppercase_x_hex() {
    assert_eq!(eval_string(r#"sprintf("%X", 255)"#), "FF");
}

#[test]
fn sprintf_percent_o_octal() {
    assert_eq!(eval_string(r#"sprintf("%o", 8)"#), "10");
}

#[test]
fn index_with_limit_from_offset() {
    assert_eq!(eval_int(r#"index("abab", "ab", 2)"#), 2);
}

#[test]
fn rindex_overlapping_pattern() {
    assert_eq!(eval_int(r#"rindex("aaaa", "aa")"#), 2);
}

#[test]
fn scalar_parenthesized_list_is_last_element() {
    assert_eq!(eval_int("scalar (10, 20, 30)"), 30);
}

#[test]
fn sort_numeric_block_then_reverse_join() {
    assert_eq!(
        eval_string(r#"join "", rev sort { $a <=> $b } (3, 1, 2)"#),
        "321"
    );
}

#[test]
fn length_builtin_on_ascii_string() {
    assert_eq!(eval_int(r#"length("abcd")"#), 4);
}

#[test]
fn index_empty_needle_reports_zero() {
    assert_eq!(eval_int(r#"index("x", "")"#), 0);
}

#[test]
fn lc_all_four_chars() {
    assert_eq!(eval_string(r#"lc("AbCd")"#), "abcd");
}

#[test]
fn uc_all_four_chars() {
    assert_eq!(eval_string(r#"uc("AbCd")"#), "ABCD");
}

#[test]
fn quotemeta_digit_preserves() {
    assert_eq!(eval_string(r#"quotemeta("9")"#), "9");
}

#[test]
fn sqrt_nine_is_three() {
    assert_eq!(eval_int("sqrt(9)"), 3);
}

#[test]
fn atan2_zero_one_radians_hint() {
    assert_eq!(eval_string(r#"sprintf("%.1f", atan2(0, 1))"#), "0.0");
}

#[test]
fn exp_log_roundtrip_one() {
    assert_eq!(eval_string(r#"sprintf("%.0f", log(exp(2)))"#), "2");
}

#[test]
fn rand_with_srand_zero_is_deterministic_twice() {
    let a = eval_string(r#"srand(999); sprintf("%.12f", rand(1))"#);
    let b = eval_string(r#"srand(999); sprintf("%.12f", rand(1))"#);
    assert_eq!(a, b);
}
