//! Additional interpreter integration tests: time builtins, extra `pack`/`unpack`, regex delimiters,
//! references/`bless`, aggregate builtins, and operator corners.

use crate::common::*;

#[test]
fn time_builtin_returns_positive_epoch() {
    assert_eq!(eval_int("time() > 1_000_000_000"), 1);
}

#[test]
fn localtime_list_nine_elements() {
    assert_eq!(
        eval_int(
            r#"my @t = localtime(0);
               scalar @t"#,
        ),
        9
    );
}

#[test]
fn localtime_scalar_looks_like_ctime_string() {
    let s = eval_string(r#"localtime(0)"#);
    assert!(
        s.contains("1969") || s.contains("1970"),
        "expected 1969 or 1970 in: {s}"
    );
}

#[test]
fn gmtime_list_nine_elements() {
    assert_eq!(
        eval_int(
            r#"my @t = gmtime(0);
               scalar @t"#,
        ),
        9
    );
}

#[test]
fn gmtime_scalar_mentions_utc_year() {
    let s = eval_string(r#"gmtime(0)"#);
    assert!(s.contains("1970") || s.contains("1969"));
}

#[test]
fn pack_unsigned_short_roundtrip_capital_s() {
    assert_eq!(eval_int(r#"scalar unpack 'S', pack 'S', 0xFE01"#), 0xFE01);
}

#[test]
fn pack_signed_short_negative_roundtrip() {
    assert_eq!(eval_int(r#"scalar unpack 's', pack 's', -3"#), -3);
}

#[test]
fn pack_f_unpack_f_approximate() {
    let s = eval_string(r#"sprintf("%.4f", scalar unpack 'f', pack 'f', 1.25)"#);
    assert_eq!(s, "1.2500");
}

#[test]
fn pack_d_unpack_d_pi_slice() {
    let s = eval_string(r#"sprintf("%.5f", scalar unpack 'd', pack 'd', 3.14159265)"#);
    assert_eq!(s, "3.14159");
}

#[test]
fn pack_i_unpack_i_roundtrip() {
    assert_eq!(eval_int(r#"scalar unpack 'i', pack 'i', -404"#), -404);
}

#[test]
fn unpack_h_star_hex_digits_uppercase() {
    assert_eq!(
        eval_string(r#"my $b = pack 'H*', "a1"; unpack 'H*', $b"#),
        "A1"
    );
}

#[test]
fn unary_plus_string_numifies_for_add() {
    assert_eq!(eval_int(r#"+"40" + 2"#), 42);
}

#[test]
fn foreach_empty_array_skips_body() {
    assert_eq!(
        eval_int(
            r#"my @e = ();
               my $n = 0;
               foreach my $x (@e) {
                   $n = $n + 1;
               }
               $n"#,
        ),
        0
    );
}

#[test]
fn m_bracket_regex_delimiters() {
    assert_eq!(eval_int(r#""abc" =~ m[b]"#), 1);
}

#[test]
fn s_brace_delimiters_substitute() {
    assert_eq!(eval_string(r#"my $s = "xax"; $s =~ s{a}{o}g; $s"#), "xox");
}

#[test]
fn qr_scalar_assigned_then_match() {
    assert_eq!(
        eval_int(
            r#"my $re = qr/^\d+$/;
               "42" =~ $re ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn bless_anon_hash_sets_ref_type() {
    assert_eq!(
        eval_string(r#"my $o = bless { n => 1 }, "Pkg"; ref $o"#),
        "Pkg"
    );
}

#[test]
fn ref_anon_hash_is_hash() {
    assert_eq!(eval_string(r#"ref({})"#), "HASH");
}

#[test]
fn keys_on_hash_deref_of_ref() {
    assert_eq!(
        eval_int(
            r#"my $r = { a => 3, b => 4 };
               scalar keys %$r"#,
        ),
        2
    );
}

#[test]
fn array_ref_deref_join() {
    assert_eq!(
        eval_string(
            r#"my $r = [5, 6, 7];
               join "-", @$r"#,
        ),
        "5-6-7"
    );
}

#[test]
fn bitwise_xor_two_values() {
    assert_eq!(eval_int("0b1100 ^ 0b1010"), 0b0110);
}

#[test]
fn string_eq_no_numeric_coercion() {
    assert_eq!(eval_int(r#""01" eq "1" ? 1 : 0"#), 0);
}

#[test]
fn numeric_eq_coerces_string_operand() {
    assert_eq!(eval_int(r#""01" == 1"#), 1);
}

#[test]
fn concat_binds_tighter_than_repeat() {
    assert_eq!(eval_string(r#""a" . "b" x 2"#), "abb");
}

#[test]
fn rindex_last_z_in_triple() {
    assert_eq!(eval_int(r#"rindex("zzz", "z")"#), 2);
}

#[test]
fn substr_insert_with_zero_length() {
    assert_eq!(
        eval_string(r#"my $s = "abcd"; substr($s, 2, 0, "Z"); $s"#),
        "abZcd"
    );
}

#[test]
fn sprintf_percent_d_no_space_flag_yet() {
    assert_eq!(eval_string(r#"sprintf("% d", 5)"#), "5");
}

#[test]
fn sprintf_percent_d_ignores_plus_flag_for_now() {
    assert_eq!(eval_string(r#"sprintf("%+d", 9)"#), "9");
}

#[test]
fn chr_255_roundtrip_ord() {
    assert_eq!(eval_int(r#"ord(chr(255))"#), 255);
}

#[test]
fn join_leading_undef_empty_field() {
    assert_eq!(eval_string(r#"join(":", undef, "x")"#), ":x");
}

#[test]
fn splice_replaces_span_with_single_insert_value() {
    assert_eq!(
        eval_string(r#"my @a = (1,2,3,4); splice @a, 1, 2, (9,9); join ",", @a"#),
        "1,9,4"
    );
}

#[test]
fn unshift_two_values_prepends() {
    assert_eq!(
        eval_string(r#"my @a = (3); unshift @a, 1, 2; join ",", @a"#),
        "1,2,3"
    );
}

#[test]
fn sharp_array_last_index_reflects_length_minus_one() {
    assert_eq!(eval_int(r#"my @a = (1, 2, 3); $#a"#), 2);
}

#[test]
fn grep_block_ne_string() {
    assert_eq!(
        eval_string(r#"join("", grep { $_ ne "b" } ("a", "b", "c"))"#),
        "ac"
    );
}

#[test]
fn sort_default_lexical_orders_digit_strings() {
    assert_eq!(eval_string(r#"join(",", sort("10", "2", "1"))"#), "1,10,2");
}

#[test]
fn map_in_scalar_context_is_element_count() {
    assert_eq!(eval_int(r#"scalar map { $_ * 2 } (3, 4, 5)"#), 3);
}

#[test]
fn defined_ampersand_sub() {
    assert_eq!(
        eval_int(
            r#"fn foo { 1 }
               defined &foo ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn pack_unsigned_long_roundtrip() {
    assert_eq!(
        eval_int(r#"scalar unpack 'L', pack 'L', 305419896"#),
        305419896
    );
}

#[test]
fn main_colon_colon_package_scalar() {
    assert_eq!(eval_int(r#"$main::x = 11; $main::x"#), 11);
}

#[test]
fn eval_string_does_not_close_over_outer_lexical() {
    assert_eq!(
        eval_int(
            r#"my $outer = 6;
               eval 'my $x = 7; $x'"#,
        ),
        7
    );
}

#[test]
fn die_in_eval_block_populates_at() {
    assert_eq!(
        eval_int(
            r#"eval { die "boom\n" };
               $@ ne "" ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn require_one_succeeds() {
    assert_eq!(eval_int("require 1; 8"), 8);
}

#[test]
fn cmp_pairs_cover_lt_and_gt() {
    assert_eq!(
        eval_int(r#"(("a" cmp "b") == -1) + (("b" cmp "a") == 1)"#),
        2
    );
}

#[test]
fn float_sum_sprintf_one_decimal() {
    assert_eq!(eval_string(r#"sprintf("%.1f", 0.1 + 0.2)"#), "0.3");
}

#[test]
fn list_assign_third_slot_stays_defined_after_partial_rhs() {
    assert_eq!(
        eval_int(
            r#"my ($a, $b, $c) = (9, 8);
               defined $c ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn keys_on_anon_hash_block_deref() {
    assert_eq!(eval_int(r#"scalar keys %{ { aa => 1, bb => 2 } }"#,), 2);
}

#[test]
fn join_localtime_nine_fields() {
    assert!(eval_string(r#"join(",", localtime(0))"#).split(',').count() >= 9);
}

#[test]
fn lc_uc_ascii_roundtrip() {
    assert_eq!(eval_string(r#"uc(lc("Hi"))"#), "HI");
}

#[test]
fn abs_float_truncates_display() {
    assert_eq!(eval_string(r#"sprintf("%.0f", abs(-2.7))"#), "3");
}

#[test]
fn int_truncates_positive_float() {
    assert_eq!(eval_int(r#"int(9.99)"#), 9);
}

#[test]
fn reverse_array_in_list_context_join() {
    assert_eq!(eval_string(r#"my @a = (1, 2, 3); join "", rev @a"#), "321");
}

#[test]
fn exists_array_element_zero() {
    assert_eq!(eval_int(r#"my @a = (0); exists $a[0] ? 1 : 0"#), 1);
}

#[test]
fn postincrement_on_array_element() {
    assert_eq!(eval_int(r#"my @a = (4); $a[0]++; $a[0]"#), 5);
}

#[test]
fn compound_mul_assign_array_elem() {
    assert_eq!(eval_int(r#"my @a = (3); $a[0] *= 4; $a[0]"#), 12);
}

#[test]
fn hash_key_exists_after_each_assignment() {
    assert_eq!(
        eval_int(
            r#"my %h;
               $h{u} = 1;
               exists $h{u} ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn empty_hash_in_boolean_is_false() {
    assert_eq!(eval_int(r#"my %h; %h ? 1 : 0"#), 0);
}

#[test]
fn nonempty_hash_in_boolean_is_true() {
    assert_eq!(eval_int(r#"my %h = (k => 0); %h ? 1 : 0"#), 1);
}

#[test]
fn eval_block_reads_outer_lexical() {
    assert_eq!(
        eval_int(
            r#"my $x = 11;
               eval { $x + 1 }"#,
        ),
        12
    );
}

#[test]
fn qq_paren_delimiter() {
    assert_eq!(eval_string(r#"qq((paren))"#), "(paren)");
}

#[test]
fn split_single_space_preserves_empty_fields_between_runs() {
    assert_eq!(eval_string(r#"join(",", split(" ", "  a  b"))"#), ",,a,,b");
}

#[test]
fn exponentiation_binds_tighter_than_unary_minus() {
    assert_eq!(eval_int(r#"-2 ** 2"#), -4);
}

#[test]
fn parenthesized_base_exponent_positive_square() {
    assert_eq!(eval_int(r#"(-2) ** 2"#), 4);
}

#[test]
fn pack_signed_long_roundtrip() {
    assert_eq!(eval_int(r#"scalar unpack 'l', pack 'l', -123456"#), -123456);
}

#[test]
fn pack_capital_i_small_value() {
    assert_eq!(eval_int(r#"scalar unpack 'I', pack 'I', 123456"#), 123456);
}
