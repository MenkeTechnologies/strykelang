//! Behavior-pinning tests captured 2026-05-04.
//!
//! Each test locks an observed-stable behavior of stryke v0.11.x. The intent is to
//! catch silent regressions, not to assert that every behavior here is "correct" —
//! a few are documented in `docs/BUGS.md` as known parity gaps with Perl 5. When a
//! gap is fixed, update the corresponding test rather than deleting it.

use crate::common::*;

// ── `fn name(args) = expr` short body (commit 6b40284) ────────────────────────

#[test]
fn fn_eq_body_basic_arithmetic() {
    assert_eq!(eval_int(r#"fn add($a, $b) = $a + $b; add(2, 3)"#), 5);
}

#[test]
fn fn_eq_body_no_args() {
    assert_eq!(eval_int(r#"fn answer = 42; answer()"#), 42);
}

#[test]
fn fn_eq_body_no_args_called_bareword() {
    assert_eq!(eval_int(r#"fn answer = 42; answer"#), 42);
}

#[test]
fn fn_eq_body_returns_list_via_parens() {
    assert_eq!(
        eval_string(r#"fn lst = (1,2,3); join(",", lst())"#),
        "1,2,3"
    );
}

#[test]
fn fn_eq_body_returns_arrayref() {
    assert_eq!(
        eval_string(r#"fn lst = [1,2,3]; join(",", @{lst()})"#),
        "1,2,3"
    );
}

#[test]
fn fn_eq_body_rejects_top_level_comma() {
    let err = stryke::parse(r#"fn add($a, $b) = $a + $b, $a * $b"#);
    assert!(err.is_err(), "top-level comma after `fn ... =` must error");
}

#[test]
fn fn_eq_body_with_default_params() {
    assert_eq!(eval_int(r#"fn add($a, $b = 10) = $a + $b; add(5)"#), 15);
}

#[test]
fn fn_eq_body_recursive() {
    // `fact`/`factorial` are stryke builtins, so use a non-conflicting name.
    assert_eq!(
        eval_int(r#"fn myfact($n) = $n <= 1 ? 1 : $n * myfact($n - 1); myfact(5)"#),
        120
    );
}

#[test]
fn fn_eq_body_closure_capture() {
    assert_eq!(
        eval_int(r#"my $base = 100; fn add5 = $base + 5; add5()"#),
        105
    );
}

#[test]
fn fn_eq_body_package_qualified() {
    assert_eq!(
        eval_int(r#"fn Foo::Bar::triple($x) = $x * 3; Foo::Bar::triple(7)"#),
        21
    );
}

// ── Perl magic string increment (PARITY-001 FIXED) ────────────────────────────
//
// `++` on a string that matches `^[A-Za-z]+[0-9]*$` (or empty) magic-
// increments through letters and digits with carry. Pure-digit strings,
// mixed strings (digits-then-letters, embedded punctuation, etc.), undef,
// and real numerics all fall back to plain numeric +1.

#[test]
fn postfix_inc_on_alpha_string_advances_letter() {
    assert_eq!(eval_string(r#"my $x = "b"; $x++; $x"#), "c");
}

#[test]
fn postfix_inc_on_alphanumeric_string_carries_through_letters() {
    assert_eq!(eval_string(r#"my $x = "Az"; $x++; $x"#), "Ba");
}

#[test]
fn postfix_inc_on_z_carries_to_double_letter() {
    assert_eq!(eval_string(r#"my $x = "zz"; $x++; $x"#), "aaa");
    assert_eq!(eval_string(r#"my $x = "ZZ"; $x++; $x"#), "AAA");
    assert_eq!(eval_string(r#"my $x = "Zz"; $x++; $x"#), "AAa");
}

#[test]
fn postfix_inc_with_digit_suffix_carries_to_letter() {
    assert_eq!(eval_string(r#"my $x = "a9"; $x++; $x"#), "b0");
    assert_eq!(eval_string(r#"my $x = "Az9"; $x++; $x"#), "Ba0");
    assert_eq!(eval_string(r#"my $x = "zz9"; $x++; $x"#), "aaa0");
}

#[test]
fn postfix_inc_on_empty_string_yields_one() {
    assert_eq!(eval_string(r#"my $x = ""; $x++; $x"#), "1");
}

#[test]
fn postfix_inc_on_pure_digit_string_increments_numerically() {
    assert_eq!(eval_int(r#"my $x = "9"; $x++; $x"#), 10);
    assert_eq!(eval_int(r#"my $x = "99"; $x++; $x"#), 100);
}

#[test]
fn postfix_inc_on_mixed_or_punctuated_string_falls_back_to_numeric() {
    // Strings that don't match the magic pattern (digits before letters,
    // embedded punctuation, leading whitespace) numify to 0 and increment
    // to 1.
    assert_eq!(eval_int(r#"my $x = "9a"; $x++; $x"#), 10);
    assert_eq!(eval_int(r#"my $x = "a9b"; $x++; $x"#), 1);
    assert_eq!(eval_int(r#"my $x = "abc_"; $x++; $x"#), 1);
}

#[test]
fn pre_inc_on_alpha_string_advances_letter() {
    assert_eq!(eval_string(r#"my $x = "B"; ++$x"#), "C");
}

#[test]
fn dec_has_no_magic_form_and_numifies() {
    // `--` always numifies; "A" → 0 → -1.
    assert_eq!(eval_int(r#"my $x = "A"; --$x"#), -1);
}

#[test]
fn inc_on_undef_yields_one() {
    assert_eq!(eval_int(r#"my $x; $x++; $x"#), 1);
}

#[test]
fn inc_on_int_value_stays_numeric() {
    assert_eq!(eval_int(r#"my $x = 5; $x++; $x"#), 6);
}

// ── `(my $copy = $orig) =~ s///` idiom (PARITY-002 FIXED) ─────────────────────
//
// The Perl idiom: declare `$copy`, initialize from `$orig`, then run the
// substitution / transliteration on `$copy` only — `$orig` stays untouched.

#[test]
fn copy_on_bind_substitute_mutates_copy_only() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; (my $t = $s) =~ s/a/X/; "$s/$t""#),
        "abc/Xbc"
    );
}

#[test]
fn copy_on_bind_tr_mutates_copy_only() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; (my $t = $s) =~ tr/a-z/A-Z/; "$s/$t""#),
        "abc/ABC"
    );
}

#[test]
fn copy_on_bind_global_substitute_replaces_all_in_copy() {
    assert_eq!(
        eval_string(r#"my $s = "abcabc"; (my $t = $s) =~ s/a/X/g; "$s/$t""#),
        "abcabc/XbcXbc"
    );
}

#[test]
fn copy_on_bind_substitute_with_capture_backref() {
    assert_eq!(
        eval_string(r#"my $s = "x"; (my $t = $s) =~ s/(.+)/[$1]/; "$s/$t""#),
        "x/[x]"
    );
}

#[test]
fn explicit_copy_then_substitute_works() {
    // Sanity check: the explicit form behaves correctly (and continues to).
    assert_eq!(
        eval_string(r#"my $s = "abc"; my $t = $s; $t =~ s/a/X/; "$s/$t""#),
        "abc/Xbc"
    );
}

#[test]
fn explicit_copy_then_tr_works() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; my $t = $s; $t =~ tr/a-z/A-Z/; "$s/$t""#),
        "abc/ABC"
    );
}

// ── `clamp` direct-call signature (MIN, MAX, LIST...) ────────────────────────

#[test]
fn clamp_direct_above_max() {
    assert_eq!(eval_int("clamp(0, 10, 15)"), 10);
}

#[test]
fn clamp_direct_below_min() {
    assert_eq!(eval_int("clamp(0, 10, -5)"), 0);
}

#[test]
fn clamp_direct_within_range() {
    assert_eq!(eval_int("clamp(0, 10, 5)"), 5);
}

#[test]
fn clamp_direct_multi_value_list() {
    assert_eq!(
        eval_string(r#"join(",", clamp(2, 4, 1, 5, 3, 6))"#),
        "2,4,3,4"
    );
}

// ── Builtin shadowing rejection ───────────────────────────────────────────────
//
// stryke refuses to redefine a builtin via `sub`/`fn` outside `--compat`.

#[test]
fn redefining_builtin_id_is_rejected() {
    let res = stryke::parse(r#"sub id { $_[0] }"#)
        .and_then(|p| stryke::interpreter::Interpreter::new().execute(&p));
    assert!(res.is_err(), "redefining `id` (a stryke builtin) must error");
}

#[test]
fn redefining_builtin_squared_is_rejected() {
    let res = stryke::parse(r#"fn squared($x) = $x * $x"#)
        .and_then(|p| stryke::interpreter::Interpreter::new().execute(&p));
    assert!(
        res.is_err(),
        "redefining `squared` (a stryke builtin) must error"
    );
}

// ── `succ`/`pred`/`signum`/`abs` numeric semantics ────────────────────────────

#[test]
fn succ_increments_int() {
    assert_eq!(eval_int("succ(4)"), 5);
}

#[test]
fn pred_decrements_int() {
    assert_eq!(eval_int("pred(4)"), 3);
}

#[test]
fn succ_on_string_numifies_to_zero_plus_one() {
    // stryke `succ` is numeric-only — stringy input numifies to 0 then succ → 1.
    assert_eq!(eval_int(r#"succ("b")"#), 1);
    assert_eq!(eval_int(r#"succ("Az")"#), 1);
}

#[test]
fn signum_three_way() {
    assert_eq!(eval_string(r#"join(",", signum(-7), signum(0), signum(7))"#), "-1,0,1");
}

#[test]
fn abs_negative() {
    assert_eq!(eval_int("abs(-7)"), 7);
}

#[test]
fn cubed_pipe_and_direct() {
    assert_eq!(eval_int("cubed(3)"), 27);
    assert_eq!(eval_int("squared(5)"), 25);
}

// ── `gcd`/`lcm`/`min`/`max` ───────────────────────────────────────────────────

#[test]
fn gcd_and_lcm() {
    assert_eq!(eval_int("gcd(12, 18)"), 6);
    assert_eq!(eval_int("lcm(4, 6)"), 12);
}

#[test]
fn min_max_variadic() {
    assert_eq!(eval_int("min(3, 1, 2)"), 1);
    assert_eq!(eval_int("max(3, 1, 2)"), 3);
}

// ── `1/0` raises an error ────────────────────────────────────────────────────
//
// Today stryke reports `1 / 0` as `ErrorKind::Runtime` even though
// `ErrorKind::DivisionByZero` exists as a variant. The error message text is
// "Illegal division by zero". Tracked in `docs/BUGS.md` (PARITY-004).

#[test]
fn division_by_zero_is_runtime_error_today() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind("1 / 0");
    assert!(
        matches!(kind, ErrorKind::Runtime),
        "current behavior: division-by-zero surfaces as Runtime, got {:?}",
        kind
    );
}

// ── `wantarray` three-way context detection ───────────────────────────────────

#[test]
fn wantarray_list_scalar_void() {
    assert_eq!(
        eval_string(
            r#"fn ctx { wantarray ? "list" : defined(wantarray) ? "scalar" : "void" }
               my @a = ctx(); my $s = ctx(); ctx();
               "@a/$s""#
        ),
        "list/scalar"
    );
}

// ── Reference deref forms agree ───────────────────────────────────────────────

#[test]
fn array_ref_deref_forms() {
    let code = r#"my @a = (10,20,30); my $r = \@a;
                  join(",", ${$r}[1], $$r[1], $r->[1], scalar(@$r))"#;
    assert_eq!(eval_string(code), "20,20,20,3");
}

// ── Closure semantics: `for my $i` captures per-iteration ────────────────────

#[test]
fn for_loop_closures_capture_own_variable() {
    let code = r#"my @cs;
                  for my $i (1..3) { push @cs, fn { $i } }
                  join(",", map { $_->() } @cs)"#;
    assert_eq!(eval_string(code), "1,2,3");
}

// ── Bigint promotion for `2 ** 64` falls back to float (stryke v0.11.x) ──────
//
// Perl 5 with `use bigint;` keeps exact integer; stryke today emits scientific
// notation. Tracked in `docs/BUGS.md` (PARITY-003).

#[test]
fn pow_2_64_uses_float_notation() {
    assert_eq!(eval_string("2 ** 64"), "1.84467440737096e+19");
}

// ── `print` vs `say` vs `p` and the list-flatten contract ────────────────────

#[test]
fn print_default_separator_is_empty() {
    // No $, set; print concatenates list items.
    assert_eq!(eval_string(r#"my @a=(1,2,3); my $s=""; for (@a){$s.=$_} $s"#), "123");
}

#[test]
fn array_in_double_quoted_uses_dollar_comma_default() {
    // $" defaults to a single space.
    assert_eq!(eval_string(r#"my @a=(1,2,3); "@a""#), "1 2 3");
}

#[test]
fn scalar_at_array_returns_count() {
    assert_eq!(eval_int("my @a = (10,20,30); scalar @a"), 3);
}

#[test]
fn last_index_dollar_hash() {
    assert_eq!(eval_int("my @a = (10,20,30); $#a"), 2);
}

// ── `eval { die HASH }` preserves reference in `$@` ──────────────────────────

#[test]
fn eval_die_with_hashref_preserves_ref() {
    assert_eq!(
        eval_int(r#"eval { die { code => 42 } }; $@->{code}"#),
        42
    );
    assert_eq!(
        eval_string(r#"eval { die { code => 42 } }; ref $@"#),
        "HASH"
    );
}

// ── String/numeric comparators ───────────────────────────────────────────────

#[test]
fn cmp_three_way_strings() {
    assert_eq!(eval_int(r#""abc" cmp "abd""#), -1);
    assert_eq!(eval_int(r#""abc" cmp "abc""#), 0);
    assert_eq!(eval_int(r#""abd" cmp "abc""#), 1);
}

#[test]
fn spaceship_three_way_numbers() {
    assert_eq!(eval_int("10 <=> 5"), 1);
    assert_eq!(eval_int("5 <=> 10"), -1);
    assert_eq!(eval_int("5 <=> 5"), 0);
}

// ── `exists` vs `defined` on hash with undef value ───────────────────────────

#[test]
fn exists_distinct_from_defined_for_undef_value() {
    assert_eq!(eval_int(r#"my %h = (a => undef); exists $h{a} ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my %h = (a => undef); defined $h{a} ? 1 : 0"#), 0);
}

// ── `qw//` produces a list ────────────────────────────────────────────────────

#[test]
fn qw_basic_list() {
    assert_eq!(
        eval_string(r#"join("-", qw(alpha beta gamma))"#),
        "alpha-beta-gamma"
    );
}

// ── Regex captures `$1`/`$2` populate after a successful match ───────────────

#[test]
fn regex_two_captures() {
    assert_eq!(
        eval_string(r#""hello world" =~ /(\w+) (\w+)/; "$1-$2""#),
        "hello-world"
    );
}

#[test]
fn regex_substitution_global() {
    assert_eq!(
        eval_string(r#"my $s = "hello"; $s =~ s/l/L/g; $s"#),
        "heLLo"
    );
}

#[test]
fn regex_substitution_backrefs_reorder() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; $s =~ s/(.)(.)(.)/$3$2$1/; $s"#),
        "cba"
    );
}

// ── `sprintf` format specifiers ──────────────────────────────────────────────

#[test]
fn sprintf_zero_padded_integer() {
    assert_eq!(eval_string(r#"sprintf("%05d", 42)"#), "00042");
}

#[test]
fn sprintf_fixed_decimal() {
    assert_eq!(eval_string(r#"sprintf("%.3f", 3.14159)"#), "3.142");
}

#[test]
fn sprintf_left_justified_string() {
    assert_eq!(
        eval_string(r#"sprintf("%-10s|%s", "left", "right")"#),
        "left      |right"
    );
}

#[test]
fn sprintf_hex_octal_binary() {
    assert_eq!(
        eval_string(r#"sprintf("%x %o %b", 255, 8, 5)"#),
        "ff 10 101"
    );
}

// ── `split` ───────────────────────────────────────────────────────────────────

#[test]
fn split_on_comma_pattern() {
    assert_eq!(
        eval_string(r#"join("|", split(/,/, "a,b,c"))"#),
        "a|b|c"
    );
}

#[test]
fn split_empty_pattern_splits_into_chars() {
    assert_eq!(
        eval_string(r#"join("|", split(//, "abc"))"#),
        "a|b|c"
    );
}

// ── Anonymous sub via `sub` and `fn` keywords are equivalent at call site ────

#[test]
fn anon_sub_via_sub_keyword() {
    assert_eq!(eval_int(r#"my $r = sub { 42 }; $r->()"#), 42);
}

#[test]
fn anon_sub_via_fn_keyword() {
    assert_eq!(eval_int(r#"my $r = fn { 42 }; $r->()"#), 42);
}

#[test]
fn anon_sub_call_via_ampersand_deref() {
    assert_eq!(eval_int(r#"my $r = fn { 42 }; &$r"#), 42);
}
