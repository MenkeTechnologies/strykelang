//! Deeper end-to-end runtime coverage: loop `continue`/`redo`, `state`, `vec`, extra `pack`/`unpack`,
//! aggregate edges, and string/numeric corners that are easy to regress.

use crate::common::*;

use stryke::error::ErrorKind;

// ── loop control: `redo` / `continue` (VM + tree paths via `eval`) ──

#[test]
fn redo_while_skips_condition_retest_once() {
    assert_eq!(
        eval_int(
            r#"my $x = 0;
               while ($x < 10) {
                   $x++;
                   if ($x == 1) { redo; }
                   last;
               }
               $x"#,
        ),
        2
    );
}

#[test]
fn while_continue_runs_after_each_iteration() {
    assert_eq!(
        eval_int(
            r#"my $i = 0;
               my $sum = 0;
               while ($i < 3) {
                   $i = $i + 1;
                   $sum = $sum + $i;
               } continue {
                   $sum = $sum + 10;
               }
               $sum"#,
        ),
        36
    );
}

#[test]
fn foreach_continue_runs_after_each_element() {
    assert_eq!(
        eval_int(
            r#"my $s = 0;
               foreach my $x (1, 2, 3) {
                   $s = $s + $x;
               } continue {
                   $s = $s + 100;
               }
               $s"#,
        ),
        306
    );
}

#[test]
fn labeled_next_outer_loop_sum() {
    // Skips the rest of the inner list when `$j == 20`: each outer pass contributes `10` only.
    assert_eq!(
        eval_int(
            r#"my $t = 0;
               OUT: foreach my $i (1, 2) {
                   foreach my $j (10, 20, 30) {
                       next OUT if $j == 20;
                       $t = $t + $j;
                   }
               }
               $t"#,
        ),
        20
    );
}

#[test]
fn labeled_last_breaks_outer() {
    assert_eq!(
        eval_int(
            r#"my $n = 0;
               OUT: foreach my $a (1..5) {
                   foreach my $b (1..5) {
                       $n = $n + 1;
                       last OUT if $n == 3;
                   }
               }
               $n"#,
        ),
        3
    );
}

// ── `state` (persists across sub calls) ──

#[test]
fn state_scalar_initializer_runs_once_across_calls() {
    assert_eq!(
        eval_int(
            r#"use feature 'state';
               fn tick {
                   state $n = 0;
                   $n++;
               }
               tick() + tick() + tick()"#,
        ),
        3
    );
}

#[test]
fn state_preserves_nonzero_initializer() {
    assert_eq!(
        eval_int(
            r#"use feature 'state';
               fn base {
                   state $n = 100;
                   $n++;
               }
               base() + base()"#,
        ),
        201
    );
}

// ── `vec` ──

#[test]
fn vec_reads_first_byte_as_eight_bits() {
    assert_eq!(eval_int(r#"vec("A", 0, 8)"#), 65);
}

#[test]
fn vec_out_of_range_returns_zero() {
    assert_eq!(eval_int(r#"vec("ab", 99, 8)"#), 0);
}

#[test]
fn vec_two_bit_field() {
    assert_eq!(eval_int(r#"vec("\x03", 0, 2)"#), 3);
}

// ── extra `pack` / `unpack` (beyond `pack_unpack_runtime`) ──

#[test]
fn pack_n_unsigned_big_endian_roundtrip() {
    assert_eq!(eval_int(r#"scalar unpack 'n', pack 'n', 0x1234"#), 0x1234);
}

#[test]
fn pack_v_unsigned_le_roundtrip() {
    assert_eq!(eval_int(r#"scalar unpack 'v', pack 'v', 0x3412"#), 0x3412);
}

#[test]
fn unpack_h_star_hex_digits_uppercase() {
    assert_eq!(
        eval_string(r#"my $b = pack 'C', 255; unpack 'H*', $b"#),
        "FF"
    );
}

#[test]
fn pack_w_ber_integer_small() {
    assert_eq!(eval_int(r#"scalar unpack 'w', pack 'w', 127"#), 127);
}

// ── aggregates: slices, `$#`, `delete` list ──

#[test]
fn array_last_index_sharp() {
    assert_eq!(eval_int(r#"my @a = qw(a b c); $#a"#), 2);
}

#[test]
fn array_slice_assign_two_slots() {
    assert_eq!(
        eval_string(r#"my @a = (0, 0, 0); @a[0, 2] = (7, 9); join(",", @a)"#),
        "7,0,9"
    );
}

#[test]
fn delete_array_element_returns_removed_scalar() {
    assert_eq!(eval_int(r#"my @a = (10, 20, 30); delete $a[2]"#), 30);
}

#[test]
fn exists_array_negative_index() {
    assert_eq!(eval_int(r#"my @a = (1); exists $a[-1] ? 1 : 0"#), 1);
}

#[test]
fn hash_slice_list_assign() {
    assert_eq!(
        eval_string(
            r#"my %h;
               @h{'a', 'b'} = (1, 2);
               $h{a} + $h{b}"#,
        ),
        "3"
    );
}

// ── numeric / string edges ──

#[test]
fn power_zero_to_zero_is_one() {
    assert_eq!(eval_int("0 ** 0"), 1);
}

#[test]
fn power_negative_exponent_integer() {
    assert_eq!(eval_string(r#"sprintf("%.3f", 2 ** -3)"#), "0.125");
}

#[test]
fn spaceship_string_vs_number_coerces() {
    assert_eq!(eval_int(r#"5 <=> "5.0""#), 0);
}

#[test]
fn string_repeat_large_then_take_length() {
    assert_eq!(eval_int(r#"length("ab" x 100)"#), 200);
}

#[test]
fn compound_concat_assign() {
    assert_eq!(
        eval_string(r#"my $s = "a"; $s .= "b"; $s .= "c"; $s"#),
        "abc"
    );
}

#[test]
fn compound_bitwise_and_assign() {
    assert_eq!(eval_int(r#"my $x = 0b1111; $x &= 0b1010; $x"#), 0b1010);
}

#[test]
fn compound_bitwise_or_assign() {
    assert_eq!(eval_int(r#"my $x = 0b1000; $x |= 0b0001; $x"#), 0b1001);
}

#[test]
fn compound_bitwise_xor_assign() {
    assert_eq!(eval_int(r#"my $x = 0b1111; $x ^= 0b1010; $x"#), 0b0101);
}

#[test]
fn left_shift_positive() {
    assert_eq!(eval_int("1 << 16"), 65536);
}

#[test]
fn atan2_negative_x_axis() {
    let s = eval_string(r#"sprintf("%.5f", atan2(0, -1))"#);
    assert_eq!(s, "3.14159");
}

#[test]
fn log_one_and_exp_zero() {
    assert_eq!(eval_string(r#"sprintf("%.1f", log(1))"#), "0.0");
    assert_eq!(eval_string(r#"sprintf("%.1f", exp(0))"#), "1.0");
}

// ── `split` / `join` corners ──

#[test]
fn split_empty_pattern_splits_characters() {
    // Perl 5: `split //, "xy"` → ("x","y") — no leading or trailing empties
    // when LIMIT is omitted/zero. (Previously stryke emitted `|x|y|` from the
    // raw regex engine; we now match Perl exactly. See `vm.rs::Op::Split`.)
    assert_eq!(eval_string(r#"join("|", split //, "xy")"#), "x|y");
    // LIMIT < 0 preserves the end-of-string match as a trailing empty.
    assert_eq!(eval_string(r#"join("|", split //, "xy", -1)"#), "x|y|");
    // Empty input → empty list.
    assert_eq!(eval_string(r#"scalar(split //, "")"#), "0");
}

#[test]
fn split_limit_one_returns_whole_string() {
    assert_eq!(eval_string(r#"join("-", split(",", "a,b,c", 1))"#), "a,b,c");
}

#[test]
fn split_negative_limit_keeps_trailing_empties() {
    // LIMIT < 0 ⇒ no truncation, trailing empties preserved (Perl 5).
    assert_eq!(
        eval_string(r#"join("|", split(/,/, "a,b,,", -1))"#),
        "a|b||"
    );
    // Default / 0 ⇒ trailing empties stripped.
    assert_eq!(eval_string(r#"join("|", split(/,/, "a,b,,"))"#), "a|b");
}

#[test]
fn list_repetition_replicates_a_paren_list() {
    // `(EXPR) x N` is list repetition (Perl). `EXPR x N` (no parens) is scalar
    // string repetition. The parser distinguishes via paren-close position; see
    // `parser.rs` `Token::X` and `compiler.rs` `ExprKind::Repeat`.
    assert_eq!(eval_string(r#"join(",", (0) x 5)"#), "0,0,0,0,0");
    assert_eq!(eval_string(r#"join(",", (0, 1) x 3)"#), "0,1,0,1,0,1");
    assert_eq!(eval_string(r#"scalar(() x 5)"#), "0"); // empty list
    assert_eq!(eval_string(r#"join(",", (1, 2, 3) x 1)"#), "1,2,3");
    // Scalar string repetition unchanged — no parens, no list-repeat.
    assert_eq!(eval_string(r#""ab" x 3"#), "ababab");
    // qw(...) is intrinsically a list constructor, no extra parens needed.
    assert_eq!(eval_string(r#"join(",", qw(a b c) x 2)"#), "a,b,c,a,b,c");
}

#[test]
fn join_undef_skips_empty_piece_between_commas() {
    assert_eq!(eval_string(r#"join(",", "a", undef, "b")"#), "a,,b");
}

// ── regex ──

#[test]
fn match_reset_digit_vars_between_matches() {
    assert_eq!(
        eval_int(
            r#"my $s = "a1b2";
               $s =~ /(\d)/;
               my $f = $1;
               $s =~ /b(\d)/;
               $f + $1"#,
        ),
        3
    );
}

#[test]
fn substitution_global_replaces_all() {
    assert_eq!(eval_string(r#"my $s = "aaa"; $s =~ s/a/b/g; $s"#), "bbb");
}

#[test]
fn transliterate_explicit_chars() {
    assert_eq!(
        eval_string(r#"my $s = "aBc"; $s =~ tr/abc/ABC/; $s"#),
        "ABC"
    );
}

// ── `eval` / errors ──

#[test]
fn eval_empty_string_is_undef_in_defined_check() {
    assert_eq!(eval_int(r#"defined(eval "") ? 1 : 0"#), 0);
}

#[test]
fn vec_illegal_bits_is_runtime_error() {
    assert_eq!(eval_err_kind(r#"vec("x", 0, 3)"#), ErrorKind::Runtime);
}

#[test]
fn division_by_zero_is_runtime_error() {
    assert_eq!(eval_err_kind("1/0"), ErrorKind::Runtime);
}

// ── subs / context ──

#[test]
fn sub_prototype_ignored_at_runtime_one_arg() {
    assert_eq!(
        eval_int(r#"fn foo ($) { my $x = shift @_; $x + 1 } foo(41)"#),
        42
    );
}

#[test]
fn return_in_sub_exits_before_following_statement() {
    assert_eq!(eval_int(r#"fn early { return 7; 99 } early()"#), 7);
}

#[test]
fn wantarray_false_in_scalar_sub_call() {
    assert_eq!(
        eval_int(
            r#"fn ctx { wantarray ? 1 : 2 }
               ctx()"#,
        ),
        2
    );
}

// ── more control flow, aggregates, builtins ──

#[test]
fn foreach_redo_reruns_body_until_counter_reaches_three() {
    assert_eq!(
        eval_int(
            r#"my $c = 0;
               foreach my $x (1) {
                   $c++;
                   if ($c < 3) { redo; }
               }
               $c"#,
        ),
        3
    );
}

#[test]
fn until_continue_adds_after_each_test() {
    assert_eq!(
        eval_int(
            r#"my $i = 0;
               my $s = 0;
               until ($i >= 2) {
                   $i = $i + 1;
                   $s = $s + $i;
               } continue {
                   $s = $s + 100;
               }
               $s"#,
        ),
        203
    );
}

#[test]
fn c_style_for_infinite_breaks_with_last() {
    assert_eq!(
        eval_int(
            r#"my $n = 0;
               for (;;) {
                   $n++;
                   last if $n == 4;
               }
               $n"#,
        ),
        4
    );
}

#[test]
fn splice_negative_offset_removes_second_to_last() {
    assert_eq!(
        eval_string(r#"my @a = (0, 1, 2, 3, 4); splice @a, -2, 1; join(",", @a)"#),
        "0,1,2,4"
    );
}

#[test]
fn array_copy_is_independent_for_push() {
    assert_eq!(
        eval_int(r#"my @a = (1); my @b = @a; push @b, 2; scalar @a + scalar @b"#),
        3
    );
}

#[test]
fn ref_scalar_returns_scalar() {
    assert_eq!(eval_string(r#"my $x = 1; ref(\$x)"#), "SCALAR");
}

#[test]
fn scalar_deref_roundtrip() {
    assert_eq!(eval_int(r#"my $x = 42; my $r = \$x; $$r"#), 42);
}

#[test]
fn wantarray_true_in_list_assignment() {
    assert_eq!(
        eval_int(
            r#"fn L { wantarray ? 7 : 0 }
               my @x = L();
               scalar @x"#,
        ),
        1
    );
}

#[test]
fn index_three_arg_skips_prefix() {
    assert_eq!(eval_int(r#"index("xxabc", "abc", 1)"#), 2);
}

#[test]
fn ord_utf8_literal_non_ascii() {
    assert!(eval_int(r#"ord("α")"#) > 127);
}

#[test]
fn sprintf_binary_percent_b() {
    assert_eq!(eval_string(r#"sprintf("%b", 5)"#), "101");
}

#[test]
fn unpack_a_reads_one_byte_from_pack_c() {
    assert_eq!(eval_string(r#"my $b = pack 'C', 65; unpack 'a', $b"#), "A");
}

#[test]
fn grep_block_substring_test() {
    assert_eq!(
        eval_string(r#"join("", grep { index($_, "a") >= 0 } ("x", "a", "ya"))"#),
        "aya"
    );
}

#[test]
fn map_expr_comma_doubles() {
    assert_eq!(eval_string(r#"join("-", map $_ * 2, (1, 2, 3))"#), "2-4-6");
}

#[test]
fn keys_empty_hash_scalar_zero() {
    assert_eq!(eval_int(r#"my %h; scalar keys %h"#), 0);
}

#[test]
fn hash_list_assign_two_entries_sum() {
    assert_eq!(eval_int(r#"my %h = ('a', 5, 'b', 6); $h{a} + $h{b}"#), 11);
}

#[test]
fn postdecrement_returns_prior() {
    assert_eq!(eval_int(r#"my $x = 5; $x--"#), 5);
}

#[test]
fn preincrement_on_hash_value() {
    assert_eq!(eval_int(r#"my %h = (k => 9); ++$h{k}; $h{k}"#), 10);
}

#[test]
fn oct_three_digit_all_ones_byte() {
    assert_eq!(eval_int(r#"oct("377")"#), 255);
}

#[test]
fn defined_hash_key_after_delete() {
    assert_eq!(
        eval_int(
            r#"my %h = (z => 1);
               delete $h{z};
               exists $h{z} ? 1 : 0"#,
        ),
        0
    );
}

#[test]
fn uc_lc_roundtrip_utf8() {
    assert_eq!(eval_string(r#"lc(uc("β"))"#), "β");
}
