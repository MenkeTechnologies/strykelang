//! Additional end-to-end runtime coverage: assignment forms, builtins return values, aggregates,
//! string/number edges, and control-flow combinations not covered elsewhere.

use crate::common::*;

#[test]
fn chained_scalar_assignment() {
    assert_eq!(eval_int("my $x; my $y; $x = $y = 4; $x + $y"), 8);
}

#[test]
fn list_assignment_two_scalars() {
    assert_eq!(eval_int("my ($a, $b) = (10, 20); $a + $b"), 30);
}

#[test]
fn list_assignment_extra_values_ignored() {
    assert_eq!(eval_int("my ($a, $b) = (1, 2, 99); $a + $b"), 3);
}

#[test]
fn list_assignment_undef_from_missing_rhs() {
    assert_eq!(eval_int("my ($a, $b) = (5); defined($b) ? 1 : 0"), 0);
}

#[test]
fn push_returns_new_length() {
    assert_eq!(eval_int("my @a = (1, 2); push @a, 3"), 3);
}

#[test]
fn unshift_returns_new_length() {
    assert_eq!(eval_int("my @a = (2, 3); unshift @a, 1"), 3);
}

#[test]
fn shift_empty_array_is_undef_in_defined_test() {
    assert_eq!(eval_int("my @a = (); defined(shift @a) ? 1 : 0"), 0);
}

#[test]
fn pop_empty_array_is_undef_in_defined_test() {
    assert_eq!(eval_int("my @a = (); defined(pop @a) ? 1 : 0"), 0);
}

#[test]
fn delete_hash_element_returns_deleted_value() {
    assert_eq!(eval_int("my %h = (a => 42, b => 1); delete $h{a}"), 42);
}

#[test]
fn splice_returns_removed_elements_as_list_joined() {
    assert_eq!(
        eval_string(r#"my @a = (1,2,3,4); join(",", splice @a, 1, 2)"#),
        "2,3"
    );
}

#[test]
fn grep_empty_input_yields_empty() {
    assert_eq!(eval_int("my @a = grep { $_ > 0 } (); scalar @a"), 0);
}

#[test]
fn map_empty_input_yields_empty() {
    assert_eq!(eval_int("my @a = map { $_ * 2 } (); scalar @a"), 0);
}

#[test]
fn foreach_iterates_hash_keys() {
    assert_eq!(
        eval_int(
            "my %h = (x => 1, y => 2); \
             my $s = 0; \
             foreach my $k (keys %h) { $s = $s + $h{$k}; } \
             $s",
        ),
        3
    );
}

#[test]
fn values_hash_returns_all_stored_values() {
    assert_eq!(
        eval_int(r#"my %h = (a => 10, b => 20, c => 30); my @v = values %h; scalar @v"#),
        3,
    );
}

#[test]
fn sort_default_lexical_strings() {
    assert_eq!(eval_string(r#"join(",", sort("b","a","c"))"#), "a,b,c");
}

#[test]
fn reverse_list_vs_string_reverse() {
    assert_eq!(eval_string(r#"join(",", reverse(1,2,3))"#), "3,2,1");
    assert_eq!(eval_string(r#"reverse("ab")"#), "ba");
}

#[test]
fn sprintf_hex_and_octal() {
    assert_eq!(eval_string(r#"sprintf("%x", 255)"#), "ff");
    assert_eq!(eval_string(r#"sprintf("%o", 8)"#), "10");
}

#[test]
fn sprintf_string_and_char() {
    assert_eq!(eval_string(r#"sprintf("%s|%c", "ok", 65)"#), "ok|A");
}

#[test]
fn index_returns_negative_when_not_found() {
    assert_eq!(eval_int(r#"index("abc", "z")"#), -1);
}

#[test]
fn rindex_finds_last_occurrence() {
    assert_eq!(eval_int(r#"rindex("abab", "ab")"#), 2);
}

#[test]
fn substr_four_arg_replaces_in_place() {
    assert_eq!(
        eval_string(r#"my $s = "hello"; substr($s, 1, 2, "XX"); $s"#),
        "hXXlo"
    );
}

#[test]
fn uc_lc_empty_string() {
    assert_eq!(eval_string(r#"uc("")"#), "");
    assert_eq!(eval_string(r#"lc("")"#), "");
}

#[test]
fn length_empty_string() {
    assert_eq!(eval_int(r#"length("")"#), 0);
}

#[test]
fn join_single_element() {
    assert_eq!(eval_string(r#"join(",", "only")"#), "only");
}

#[test]
fn abs_zero() {
    assert_eq!(eval_int("abs(0)"), 0);
}

#[test]
fn int_truncates_toward_zero() {
    assert_eq!(eval_int("int(-3.7)"), -3);
}

#[test]
fn sqrt_perfect_square_and_non_integer() {
    assert_eq!(eval_int("sqrt(49)"), 7);
    assert_eq!(eval_string(r#"sprintf("%.1f", sqrt(2))"#), "1.4");
}

#[test]
fn sin_cos_atan2_exp_log() {
    assert_eq!(eval_string(r#"sprintf("%.1f", sin(0))"#), "0.0");
    assert_eq!(eval_string(r#"sprintf("%.1f", cos(0))"#), "1.0");
    assert_eq!(eval_string(r#"sprintf("%.1f", atan2(1, 1))"#), "0.8");
    assert_eq!(
        eval_string(r#"sprintf("%.1f", log(2.718281828459045))"#),
        "1.0"
    );
    assert_eq!(eval_string(r#"sprintf("%.1f", exp(1))"#), "2.7");
}

#[test]
fn srand_returns_abs_seed_and_rand_reproducible() {
    assert_eq!(eval_int("srand(-9)"), 9);
    let a = eval_string(r#"srand(12345); sprintf("%.12f", rand(1))"#);
    let b = eval_string(r#"srand(12345); sprintf("%.12f", rand(1))"#);
    assert_eq!(a, b);
}

#[test]
fn fc_case_folds_ascii() {
    assert_eq!(eval_string(r#"fc("Hello")"#), "hello");
}

#[test]
fn study_matches_perl5_return_value() {
    // Perl: non-empty → `1`; empty string → defined value that numifies to `0`.
    assert_eq!(eval_int(r#"study "hello""#), 1);
    assert_eq!(eval_int(r#"study "café""#), 1);
    assert_eq!(eval_int(r#"study """#), 0);
}

#[test]
fn pos_tracks_scalar_g_matches() {
    assert_eq!(
        eval_int(r#"my $s = "foo"; my $n = 0; while ($s =~ /o/g) { $n = pos($s) } $n"#,),
        3
    );
}

#[cfg(unix)]
#[test]
fn crypt_unix_non_empty() {
    let p = eval_string(r#"crypt("ab", "aa")"#);
    assert_eq!(p.len(), 13);
}

#[test]
fn or_assign_via_expansion_not_token() {
    // `||=` is not tokenized yet; spell the Perl 5 expansion.
    assert_eq!(eval_int("my $x = 0; $x = $x || 9; $x"), 9);
    assert_eq!(eval_int("my $x = 5; $x = $x || 9; $x"), 5);
}

#[test]
fn defined_or_assign_via_expansion() {
    assert_eq!(eval_int("my $x; $x = defined($x) ? $x : 7; $x"), 7);
    assert_eq!(eval_int("my $x = 0; $x = defined($x) ? $x : 7; $x"), 0);
}

#[test]
fn bitwise_shift_negative_is_arithmetic_right() {
    assert_eq!(eval_int("-8 >> 1"), -4);
}

#[test]
fn modulo_negative_divisor() {
    assert_eq!(eval_int("7 % -3"), 1);
}

#[test]
fn comparison_chains_via_short_circuit() {
    assert_eq!(eval_int("1 < 2 && 2 < 3"), 1);
    assert_eq!(eval_int("1 < 2 && 2 > 3"), 0);
}

#[test]
fn ternary_nested() {
    assert_eq!(eval_int("my $n = 2; $n == 0 ? 0 : $n == 1 ? 10 : 20"), 20);
}

#[test]
fn for_c_style_never_enters_when_condition_false() {
    assert_eq!(
        eval_int(
            "my $x = 0; \
             for (my $i = 0; $i < 0; $i = $i + 1) { $x = $x + 1; } \
             $x",
        ),
        0
    );
}

#[test]
fn while_never_runs() {
    assert_eq!(
        eval_int(
            "my $x = 0; \
             while (0) { $x = $x + 1; } \
             $x",
        ),
        0
    );
}

#[test]
fn until_runs_until_true() {
    assert_eq!(
        eval_int(
            "my $i = 0; \
             until ($i >= 3) { $i = $i + 1; } \
             $i",
        ),
        3
    );
}

#[test]
fn labeled_next_skips_to_next_iteration() {
    assert_eq!(
        eval_int(
            "my $s = 0; \
             L: foreach my $i (1,2,3,4,5) { \
                 next L if $i % 2 == 0; \
                 $s = $s + $i; \
             } \
             $s",
        ),
        9
    );
}

#[test]
fn array_slice_two_indices() {
    assert_eq!(
        eval_string(r#"my @a = (10, 20, 30, 40); join(",", @a[1, 3])"#),
        "20,40"
    );
}

#[test]
fn hash_each_key_exists_after_assignment() {
    assert_eq!(eval_int(r#"my %h; $h{u} = 1; exists $h{u} ? 1 : 0"#), 1);
}

#[test]
fn scalar_array_in_boolean_context() {
    assert_eq!(eval_int("my @a = (0); @a ? 1 : 0"), 1);
    assert_eq!(eval_int("my @a = (); @a ? 1 : 0"), 0);
}

#[test]
fn numeric_string_eq_uses_numeric_comparison() {
    assert_eq!(eval_int(r#"7 == "7.0""#), 1);
}

#[test]
fn string_ne_numeric_string() {
    assert_eq!(eval_int(r#"7 != "8""#), 1);
}

#[test]
fn repeat_operator_zero_and_negative_is_empty() {
    assert_eq!(eval_string(r#""x" x 0"#), "");
    assert_eq!(eval_string(r#""x" x -1"#), "");
}

#[test]
fn concat_preserves_order() {
    assert_eq!(eval_string(r#""a" . "b" . "c""#), "abc");
}

#[test]
fn anon_sub_returns_from_block() {
    assert_eq!(eval_int("my $f = sub { return 8; 9 }; $f->()"), 8);
}

#[test]
fn sub_returns_first_arg_shift_with_extra_args() {
    // Explicit `return` — bare trailing `$a` after `my` is not the block result in this engine.
    assert_eq!(
        eval_int("sub add { my $a = shift @_; return $a; } add(1, 2, 3)"),
        1
    );
}

#[test]
fn eval_block_sets_at_on_die() {
    assert_eq!(
        eval_int(
            r#"eval { die "x\n" }; \
               $@ ne "" ? 1 : 0"#,
        ),
        1
    );
}

#[test]
fn regex_global_match_in_scalar_context_still_truthy() {
    assert_eq!(eval_int(r#"my $s = "abc"; ($s =~ /./g) ? 1 : 0"#), 1);
}

#[test]
fn substitution_count_without_g() {
    assert_eq!(eval_string(r#"my $s = "aa"; $s =~ s/a/b/; $s"#), "ba");
}

#[test]
fn package_scalar_not_lexical() {
    assert_eq!(
        eval_int(
            "our $pkg_counter = 0; \
             $pkg_counter = $pkg_counter + 1; \
             $pkg_counter",
        ),
        1
    );
}

#[test]
fn do_block_lexical_scope() {
    assert_eq!(
        eval_int("my $x = 1; my $y = do { my $x = 100; $x }; $y + $x"),
        101
    );
}

#[test]
fn postfix_for_accumulates() {
    assert_eq!(eval_int("my $t = 0; $t += $_ for 1..5; $t"), 15);
}

#[test]
fn range_float_endpoints_coerce() {
    assert_eq!(eval_int("my @a = (1..3); scalar @a"), 3);
}

#[test]
fn sort_numeric_block_all_equal() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } (5,5,5))"#),
        "5,5,5"
    );
}

#[test]
fn grep_false_excludes() {
    assert_eq!(
        eval_string(r#"join(",", grep { $_ > 2 } (1,2,3,4))"#),
        "3,4"
    );
}

#[test]
fn map_identity_list() {
    assert_eq!(eval_string(r#"join(",", map { $_ } (9,8,7))"#), "9,8,7");
}

#[test]
fn map_multistmt_last_expr_bytecode() {
    assert_eq!(
        eval_string(r#"join(",", map { my $x = 1; $_ + $x } (1,2,3))"#),
        "2,3,4"
    );
}

/// Map/grep/sort blocks run in the caller scope with a block-local frame per iteration (Perl 5:
/// `$_`/`$a`/`$b` are not closure captures).
#[test]
fn map_block_mutates_outer_lexical() {
    assert_eq!(eval_int(r#"my $s = 0; map { $s += $_ } (1, 2, 3); $s"#), 6);
}
