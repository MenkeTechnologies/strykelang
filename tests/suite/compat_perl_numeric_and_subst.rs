//! Regression tests for four `--compat` divergences found by byte-diffing
//! `stryke --compat` against `/usr/bin/perl`.
//!
//! Everything here is gated on `--compat`, and the flag is process-global, so
//! each case runs the real `st` binary in a subprocess rather than flipping the
//! flag in-process. That is also what the parity corpus does, so a failure here
//! and a failure in `parity/cases/33{0,1,2,3}_*.pl` mean the same thing.

use std::process::Command;

/// Run `st --compat -e CODE` and return stdout, asserting the run succeeded.
fn compat(code: &str) -> String {
    let exe = env!("CARGO_BIN_EXE_st");
    let out = Command::new(exe)
        .args(["--compat", "-e", code])
        .output()
        .expect("spawn st");
    assert!(
        out.status.success(),
        "st --compat -e {code:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ── `**` follows perl's pp_pow highbit rule ─────────────────────────────────

#[test]
fn pow_stays_integer_only_inside_perls_highbit_window() {
    // `10**16`: highbit(10) == 4, 4 * 16 == 64 — exactly at the limit, so perl
    // uses integer arithmetic and prints every digit.
    assert_eq!(compat("print 10**16"), "10000000000000000");
    // `10**17`: 4 * 17 == 68 — over the limit, so C `pow()` and `%.15g`.
    assert_eq!(compat("print 10**17"), "1e+17");
    // `2**53` is smaller than `10**16` and exactly representable, yet
    // highbit(2) == 2 and 2 * 53 == 106, so it is an NV. This pair is the
    // whole reason the rule cannot be approximated by "did it overflow?".
    assert_eq!(compat("print 2**53"), "9.00719925474099e+15");
    assert_eq!(compat("print 2**31"), "2147483648");
    assert_eq!(compat("print 7**22"), "3.90982104858299e+18");
}

#[test]
fn pow_negative_base_sign_follows_exponent_parity() {
    assert_eq!(compat("my $b = -2; print $b**3"), "-8");
    assert_eq!(compat("my $b = -2; print $b**2"), "4");
    // Magnitude 2**63 does not fit an IV, so perl reports it as an NV.
    assert_eq!(compat("my $b = -2; print $b**63"), "-9.22337203685478e+18");
    assert_eq!(compat("my $b = -3; print $b**41"), "-3.64729963771708e+19");
}

#[test]
fn pow_overflow_reaches_infinity_instead_of_a_bigint() {
    // `9**9**9` is 9**387420489. Computing it exactly is not an optimization
    // problem, it is a hang — the exact value needs over a gigabit of digits.
    assert_eq!(compat("print 9**9**9"), "Inf");
    assert_eq!(compat("print 2**64"), "1.84467440737096e+19");
}

#[test]
fn pow_zero_and_fractional_exponents() {
    assert_eq!(compat("print 0**0"), "1");
    assert_eq!(compat("print 0**5"), "0");
    assert_eq!(compat("print 2**-2"), "0.25");
    assert_eq!(compat("print 2**0.5"), "1.4142135623731");
}

// ── NV stringification and SvIV_please ──────────────────────────────────────

#[test]
fn nv_switches_to_scientific_notation_at_1e15() {
    // `%.15g` goes scientific once the exponent reaches the precision.
    assert_eq!(compat("print 1e14"), "100000000000000");
    assert_eq!(compat("print 1e15"), "1e+15");
    assert_eq!(compat("print 999999999999999.0"), "999999999999999");
    assert_eq!(compat("print 1234567890123456.0"), "1.23456789012346e+15");
}

#[test]
fn negative_zero_prints_as_plain_zero() {
    // C's `%.15g` yields `-0` here; perl does not, so the integral fast path
    // has to keep covering it.
    assert_eq!(compat("my $z = -0.0; print $z"), "0");
}

#[test]
fn integral_nv_operands_make_add_sub_mul_produce_integers() {
    // perl's `SvIV_please_nomg` runs on the operands of + - * (but not /), so
    // the same value prints two different ways depending on the operator.
    assert_eq!(compat("print 1e15 + 1"), "1000000000000001");
    assert_eq!(compat("print 1e15 + 0"), "1000000000000000");
    assert_eq!(compat("print 1e15 - 0"), "1000000000000000");
    assert_eq!(compat("print 1e15 * 1"), "1000000000000000");
    assert_eq!(compat("my $x = 1e15; $x += 0; print $x"), "1000000000000000");
    // Division has no such step.
    assert_eq!(compat("print 1e15 / 1"), "1e+15");
    // Neither does stringification on its own.
    assert_eq!(compat("my $x = 1e15; print $x"), "1e+15");
}

#[test]
fn fractional_operands_keep_the_expression_floating() {
    assert_eq!(compat("print 0.1 + 0.2"), "0.3");
    assert_eq!(compat("print 1 / 3"), "0.333333333333333");
    assert_eq!(compat("print 3.14 * 2"), "6.28");
    assert_eq!(compat("print 1e300 * 1e300"), "Inf");
}

// ── s/// replacement: case escapes, `$&`, literal backslashes ───────────────

#[test]
fn case_escapes_apply_per_match_to_the_expanded_replacement() {
    // A template-level implementation emits a literal `\u`; a whole-result one
    // capitalizes only "Foo".
    assert_eq!(
        compat(r#"my $s = "foo bar"; $s =~ s/(\w+)/\u$1/g; print $s"#),
        "Foo Bar"
    );
    assert_eq!(
        compat(r#"my $s = "FOO BAR"; $s =~ s/(\w+)/\l$1/g; print $s"#),
        "fOO bAR"
    );
    assert_eq!(
        compat(r#"my $s = "foo"; $s =~ s/(\w+)/\U$1\E!/; print $s"#),
        "FOO!"
    );
    assert_eq!(
        compat(r#"my $s = "a-b"; $s =~ s/(\w)-(\w)/\U$1\E-\u$2/; print $s"#),
        "A-B"
    );
}

#[test]
fn dollar_amp_expands_to_the_whole_match() {
    assert_eq!(compat(r#"my $s = "xy"; $s =~ s/x/[$&]/; print $s"#), "[x]y");
    assert_eq!(compat(r#"my $s = "xy"; $s =~ s/x/\U$&/; print $s"#), "Xy");
}

#[test]
fn a_literal_backslash_is_not_a_case_escape() {
    // `s/x/a\\Ub/` is a backslash then `Ub`, not `\U` applied to `b`. The two
    // are the same character sequence after naive unescaping, which is why the
    // lexer has to keep `\\` doubled for the replacement layer.
    assert_eq!(compat(r#"my $s = "x"; $s =~ s/x/a\\Ub/; print $s"#), r"a\Ub");
    assert_eq!(compat(r#"my $s = "x"; $s =~ s/x/a\\b/; print $s"#), r"a\b");
    assert_eq!(compat(r#"my $s = "x"; $s =~ s/x/\\/; print $s"#), r"\");
    assert_eq!(compat(r#"my $s = "x"; $s =~ s/x/\\n/; print $s"#), r"\n");
    assert_eq!(compat(r#"my $s = "x"; $s =~ s{x}{a\\Ub}; print $s"#), r"a\Ub");
}

#[test]
fn subst_still_reports_the_match_count_and_honours_r() {
    assert_eq!(
        compat(r#"my $s = "aaa"; my $n = ($s =~ s/(a)/\u$1/g); print "$n $s""#),
        "3 AAA"
    );
    assert_eq!(
        compat(r#"my $s = "hi there"; my $r = $s =~ s/(\w+)/\u$1/gr; print "$r|$s""#),
        "Hi There|hi there"
    );
}

// ── each ────────────────────────────────────────────────────────────────────
//
// stryke iterates hashes in insertion order (perl randomizes per process), so
// these assert the set and the iterator state, never the order.

#[test]
fn each_visits_every_pair_once_then_returns_the_empty_list() {
    assert_eq!(
        compat(
            r#"my %h = (a=>1, b=>2, c=>3);
               my @p; while (my ($k,$v) = each %h) { push @p, "$k=$v" }
               print join(",", sort @p)"#
        ),
        "a=1,b=2,c=3"
    );
    assert_eq!(
        compat(r#"my %e; my @r = each %e; print scalar(@r)"#),
        "0"
    );
}

#[test]
fn exhausting_the_iterator_rewinds_it_for_the_next_loop() {
    // Two back-to-back loops must both see all three pairs. If exhaustion did
    // not rewind, the second loop would see none.
    assert_eq!(
        compat(
            r#"my %h = (a=>1, b=>2, c=>3);
               my $n = 0; while (my ($k,$v) = each %h) { $n++ }
               my $m = 0; while (my ($k,$v) = each %h) { $m++ }
               print "$n $m""#
        ),
        "3 3"
    );
}

#[test]
fn keys_and_values_reset_a_partially_consumed_iterator() {
    assert_eq!(
        compat(
            r#"my %h = (a=>1, b=>2, c=>3);
               each %h; keys %h;
               my $n = 0; while (my ($k,$v) = each %h) { $n++ }
               print $n"#
        ),
        "3"
    );
    assert_eq!(
        compat(
            r#"my %h = (a=>1, b=>2, c=>3);
               each %h; values %h;
               my $n = 0; while (my ($k,$v) = each %h) { $n++ }
               print $n"#
        ),
        "3"
    );
    // Without an intervening keys/values the iterator resumes instead.
    assert_eq!(
        compat(
            r#"my %h = (a=>1, b=>2, c=>3);
               each %h;
               my $n = 0; while (my ($k,$v) = each %h) { $n++ }
               print $n"#
        ),
        "2"
    );
}

#[test]
fn each_in_scalar_context_yields_the_key() {
    assert_eq!(
        compat(
            r#"my %h = (a=>1, b=>2);
               my @k; while (defined(my $k = each %h)) { push @k, $k }
               print join(",", sort @k)"#
        ),
        "a,b"
    );
}

#[test]
fn the_loop_ends_on_the_assignment_count_not_the_key_truth() {
    // "0" and "" are false keys. A `while ($k = each %h)` loop stops on the
    // first of them; the list form must not, because a two-element assignment
    // in scalar context is 2 regardless of what the elements are.
    assert_eq!(
        compat(
            r#"my %h = ('0'=>'zero', ''=>'empty', 'x'=>'ex');
               my $n = 0; while (my ($k,$v) = each %h) { $n++ }
               print $n"#
        ),
        "3"
    );
}

#[test]
fn list_declaration_in_expression_context_yields_the_rhs_element_count() {
    // The value `while (my ($k,$v) = each %h)` tests is the number of elements
    // the right-hand side produced — Perl's value for a list assignment in
    // scalar context. It is not the number of variables declared.
    assert_eq!(compat("my $n = (my ($a, $b) = (1, 2, 3)); print $n"), "3");
    assert_eq!(compat("my $n = (my ($a, $b) = ()); print $n"), "0");
    assert_eq!(
        compat(r#"if (my ($x, $y) = ("ab" =~ /(a)(b)/)) { print "$x$y" }"#),
        "ab"
    );
}
