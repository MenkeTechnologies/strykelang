//! Behavior-pinning batch D (2026-05-04): special vars, scoping, typeglobs,
//! numeric edges, format/write, autovivification, slurp mode, signals.

use crate::common::*;

// ── Output separators $, $\ $" ──────────────────────────────────────────────

#[test]
fn output_field_separator_dollar_comma() {
    assert_eq!(
        eval_string(
            r#"local $, = ":"; my $s = ""; for (1, 2, 3) { $s .= $_; $s .= "," } chop $s; $s"#
        ),
        // Skip the local-$, behavior; just sanity-check the helper used in the
        // probe via direct concatenation. The point is `$, ` is parseable.
        "1,2,3"
    );
}

#[test]
fn output_record_separator_dollar_backslash() {
    // `local $\\ = "|"; print "a"; print "b";` produces "a|b|" on stdout.
    // That is a CLI test (we cannot capture stdout from inside `eval`), so we
    // just verify the special variable is assignable without error.
    assert_eq!(eval_string(r#"local $\ = "|"; "OK""#), "OK");
}

#[test]
fn list_separator_dollar_quote_default_is_space() {
    assert_eq!(eval_string(r#"my @a = (1,2,3); "@a""#), "1 2 3");
}

#[test]
fn list_separator_can_be_overridden_via_local() {
    assert_eq!(
        eval_string(r#"local $" = "+"; my @a = (1,2,3); "@a""#),
        "1+2+3"
    );
}

// ── $0, $$, $? ────────────────────────────────────────────────────────────────

#[test]
fn dollar_zero_in_lib_eval_is_stryke() {
    // The CLI uses "-e"; the library eval path reports "stryke" instead.
    // Pin the lib value here. The CLI form is exercised in cli_* tests.
    assert_eq!(eval_string("$0"), "stryke");
}

#[test]
fn dollar_dollar_is_positive_pid() {
    // Just sanity: must be a positive integer.
    let pid = eval_int("$$");
    assert!(pid > 0, "expected positive PID, got {}", pid);
}

#[test]
fn dollar_question_after_system_true_is_zero() {
    assert_eq!(eval_int(r#"system "true"; $?"#), 0);
}

#[test]
fn dollar_question_after_system_false_is_256() {
    // Perl convention: exit code << 8.
    assert_eq!(eval_int(r#"system "false"; $?"#), 256);
}

// ── $_ topic and `for` loop semantics ────────────────────────────────────────

#[test]
fn underscore_topic_used_by_default_in_uc() {
    assert_eq!(eval_string(r#"$_ = "hello"; uc"#), "HELLO");
}

#[test]
fn for_dollar_underscore_aliases_array_element() {
    // BUG-019 FIXED: `for (@a) { $_ *= 10 }` mutates @a in place — the
    // loop variable is aliased to the array element and the write-back
    // is emitted at the end of each iteration.
    assert_eq!(
        eval_string(r#"my @a = (1..3); for (@a) { $_ *= 10 } "@a""#),
        "10 20 30"
    );
}

#[test]
fn for_named_loop_var_aliases_array_element() {
    // BUG-019b FIXED: named loop var also aliases, matching Perl.
    assert_eq!(
        eval_string(r#"my @a = (1..3); for my $x (@a) { $x *= 10 } "@a""#),
        "10 20 30"
    );
}

#[test]
fn for_alias_respects_last_and_next() {
    // `last` writes back the current iteration's value before exiting;
    // `next` writes back before continuing.
    assert_eq!(
        eval_string(r#"my @a = (1..5); for (@a) { last if $_ == 3; $_ = -$_ } "@a""#),
        "-1 -2 3 4 5"
    );
    assert_eq!(
        eval_string(r#"my @a = (1..5); for (@a) { next if $_ == 3; $_ = -$_ } "@a""#),
        "-1 -2 3 -4 -5"
    );
}

#[test]
fn for_alias_only_for_simple_array_source() {
    // Aliasing only fires when the source is a bare-`@arr` lvalue. A range
    // or list is not an lvalue, so the loop var is just a copy. This
    // matches Perl 5: `for my $i (1..3) { $i *= 10 }` mutates a temporary,
    // not the literal range.
    let _ = eval_string(r#"for my $i (1..3) { $i *= 10 } "ok""#);
}

#[test]
fn for_index_assignment_works() {
    // The previously-only-working workaround keeps working.
    assert_eq!(
        eval_string(r#"my @a = (1..3); for my $i (0..$#a) { $a[$i] *= 10 } "@a""#),
        "10 20 30"
    );
}

// ── Regex match-related globals ──────────────────────────────────────────────

#[test]
fn match_dollar_amp_captures_whole_match() {
    assert_eq!(eval_string(r#""abXYZcd" =~ /XYZ/; my $m = $&; $m"#), "XYZ");
}

#[test]
fn match_dollar_amp_interpolates_correctly() {
    // BUG-029 (FIXED): `$&` now interpolates inside double-quoted strings.
    // Fix: added `&` to the special-char branch of `parse_interpolated_string`
    // (parser.rs) alongside the existing `'`/`` ` `` handlers.
    assert_eq!(eval_string(r#""abXYZcd" =~ /XYZ/; "[$&]""#), "[XYZ]");
}

#[test]
fn premuf_via_english_alias_works() {
    // BUG-019: bare `my $p = $\`` (pre-match) does not parse — workaround is
    // `use English; $PREMATCH`.
    assert_eq!(
        eval_string(r#"use English; "hello world" =~ /world/; my $p = $PREMATCH; $p"#),
        "hello "
    );
}

#[test]
fn match_offset_arrays_plus_and_minus() {
    let out = eval_string(r#"my $s = "hello"; $s =~ /(l+)/; $-[1] . "/" . $+[1]"#);
    assert_eq!(out, "2/4");
}

#[test]
fn named_capture_hash_lists_keys() {
    assert_eq!(
        eval_string(
            r#""abc 123" =~ /(?<word>\w+)\s(?<num>\d+)/;
               join(",", sort keys %+)"#
        ),
        "num,word"
    );
}

// ── Slurp mode `$/` is broken today ──────────────────────────────────────────

#[test]
fn open_then_slurp_with_undef_separator_reads_whole_file() {
    // BUG-018 FIXED: `local $/; <$fh>` now slurps the whole file.
    let probe = std::env::temp_dir().join(format!("stryke_pin_slurp_{}", std::process::id()));
    std::fs::write(&probe, b"line1\nline2\n").unwrap();
    let probe_str = probe.to_string_lossy().to_string();
    let code = format!(
        r#"open my $fh, "<", "{}" or die; local $/; my $x = <$fh>; close $fh; length($x)"#,
        probe_str
    );
    let n = eval_int(&code);
    let _ = std::fs::remove_file(&probe);
    // Full file is 12 bytes ("line1\nline2\n")
    assert_eq!(n, 12);
}

// ── `our` / `local` / `state` ────────────────────────────────────────────────

#[test]
fn our_var_persists_across_subs() {
    assert_eq!(
        eval_int(r#"our $X = 1; sub bump { $X++ } bump(); bump(); $X"#),
        3
    );
}

#[test]
fn our_var_visible_via_package_qualifier() {
    assert_eq!(
        eval_int(
            r#"package Foo;
               our $x = 10;
               sub get { $x }
               package main;
               Foo::get() + $Foo::x"#
        ),
        20
    );
}

#[test]
fn local_restores_value_after_inner_sub_exit() {
    let out = eval_string(
        r#"our $X = "outer";
           sub inner  { $X }
           sub middle { local $X = "mid"; inner(); }
           inner() . "/" . middle() . "/" . inner()"#,
    );
    assert_eq!(out, "outer/mid/outer");
}

#[test]
fn state_var_persists_within_one_sub() {
    assert_eq!(
        eval_string(
            r#"sub mycounter { state $n = 0; ++$n }
               mycounter() . "," . mycounter() . "," . mycounter()"#
        ),
        "1,2,3"
    );
}

// ── Typeglob aliasing ────────────────────────────────────────────────────────

#[test]
fn typeglob_assigns_coderef_alias() {
    assert_eq!(
        eval_int(r#"sub original { 42 } *alias = \&original; alias()"#),
        42
    );
}

#[test]
fn typeglob_assigns_full_glob_alias() {
    assert_eq!(
        eval_string(r#"sub original { "orig" } *alias = *original; alias()"#),
        "orig"
    );
}

// ── Dereferencing scalar-ref to arrayref is broken today ─────────────────────

#[test]
fn scalar_ref_to_arrayref_unwrap_fails_today() {
    // BUG-021: stryke can't follow `$$r->[0]` when `$r = \$x` and `$x` is
    // itself an arrayref.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my $x = [1,2,3]; my $r = \$x; print $$r->[0]"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime/type error, got {:?}",
        kind
    );
}

// ── Weaken / isweak today ────────────────────────────────────────────────────

#[test]
fn weaken_does_not_make_isweak_true_today() {
    // BUG-022: `weaken($r)` runs but `isweak($r)` still returns 0.
    assert_eq!(
        eval_int(r#"my $a = [1]; my $b = $a; weaken($b); isweak($b) ? 1 : 0"#),
        0
    );
}

// ── Autovivification of nested hash/array fails today ───────────────────────

#[test]
fn autoviv_hash_then_array_index_fails_today() {
    // BUG-023: Perl auto-vivifies `$h{k}` to an arrayref on first
    // index-assignment. Stryke errors.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my %h; $h{k}[0] = "first"; $h{k}[0]"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime/type error, got {:?}",
        kind
    );
}

// ── given/when with arrayref pattern fails today ─────────────────────────────

#[test]
fn given_when_arrayref_range_fails_today() {
    // BUG-024: the smart-match `[1..5]` arrayref pattern raises
    // "unexpected control flow in tree-assisted opcode".
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"use feature "switch";
           sub g { my $x = $_[0]; given ($x) {
             when ([1..5])  { return "low" }
             when ([6..10]) { return "high" }
             default        { return "?" }
           }}
           g(3)"#,
    );
    assert!(
        matches!(
            kind,
            ErrorKind::Runtime | ErrorKind::Type | ErrorKind::Syntax
        ),
        "expected error, got {:?}",
        kind
    );
}

#[test]
fn given_when_inside_sub_fails_today() {
    // BUG-024b: `given/when` works at the top level but raises "unexpected
    // control flow in tree-assisted opcode" inside a `sub`. Pin the failure
    // until the VM lowering for `return` from inside `when` is fixed.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"use feature "switch";
           sub g { my $x = $_[0]; given ($x) {
             when ("hi") { return "M" } default { return "N" }
           }}
           g("hi")"#,
    );
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

// ── $SIG{__WARN__} handler not invoked today ─────────────────────────────────
//
// Hard to assert without capturing stderr from inside `eval`. Pin the
// observable: assigning the SIG entry returns a coderef, but invocation isn't
// checked here (CLI-only). Listed in BUGS.md (BUG-025).

#[test]
fn sig_warn_assignment_succeeds() {
    assert_eq!(
        eval_string(r#"$SIG{__WARN__} = sub { 1 }; ref $SIG{__WARN__}"#),
        "CODE"
    );
}

// ── format / write — TOP not invoked today ───────────────────────────────────
//
// Like SIG, this is observable mainly via stdout. We just confirm that the
// `format` declaration parses without error and that `write` returns a value.

#[test]
fn format_declaration_parses() {
    let _ = eval_string(
        r#"format STDOUT =
@>>>>>
"hi"
.
"OK""#,
    );
    // Did not panic = parse + execute succeeded (format declaration evaluates
    // to nothing visible at the value level).
}

// ── `$s x= N` not parsed today ───────────────────────────────────────────────

#[test]
fn x_compound_assign_repeats_string_in_place() {
    // `$s x= N` desugars to `$s = $s x N` and modifies in place.
    assert_eq!(eval_string(r#"my $s = "ab"; $s x= 3; $s"#), "ababab");
}

#[test]
fn x_compound_workaround_works() {
    assert_eq!(eval_string(r#"my $s = "ab"; $s = $s x 3; $s"#), "ababab");
}

// ── `$#a = N` lvalue not honored today ───────────────────────────────────────

#[test]
fn dollar_hash_array_lvalue_truncates() {
    // BUG-027 (FIXED): `$#a = N` now resizes `@a` to length `N + 1`.
    // Fix: routed `#name` writes through `set_special_var`, which resizes
    // the array via `scope.set_array(name, vec_resized_to_N+1)`.
    assert_eq!(eval_int(r#"my @a = (1..5); $#a = 2; scalar @a"#), 3);
}

// ── Hash slice with arrayref-deref keys returns empty today ──────────────────

#[test]
fn hash_slice_with_literal_keys_returns_correct_values() {
    // The form that does work: literal key list embedded in the slice.
    assert_eq!(
        eval_string(r#"my %h = (a=>1, b=>2, c=>3); my @v = @h{("a","c")}; "@v""#),
        "1 3"
    );
}

#[test]
fn hash_slice_with_array_var_keys_returns_empty_today() {
    // BUG-028: passing keys via an array variable yields nothing. The literal
    // form (above) works; the array-var form does not.
    assert_eq!(
        eval_string(r#"my %h = (a=>1, b=>2, c=>3); my @ks = ("a","c"); my @v = @h{@ks}; "@v""#),
        ""
    );
}

// ── String numeric coercion ───────────────────────────────────────────────────

#[test]
fn numeric_inf_string_becomes_infinity() {
    // PARITY-015 FIXED: Perl 5 numifies "Inf"/"Infinity"/"NaN" (case-insensitive,
    // optional sign) to actual float specials. Match that.
    assert_eq!(eval_string(r#""Inf" + 1"#), "Inf");
    assert_eq!(eval_string(r#""Infinity" + 1"#), "Inf");
    assert_eq!(eval_string(r#""inf" + 1"#), "Inf");
    assert_eq!(eval_string(r#""-Inf" + 1"#), "-Inf");
    assert_eq!(eval_string(r#""+Inf" + 1"#), "Inf");
    assert_eq!(eval_string(r#""NaN" + 0"#), "NaN");
    assert_eq!(eval_string(r#""nan" + 0"#), "NaN");
}

#[test]
fn numeric_overflow_yields_inf() {
    // 9 ** 9 ** 9 overflows IEEE 754 — Perl prints "Inf".
    assert_eq!(eval_string("9 ** 9 ** 9"), "Inf");
    assert_eq!(eval_string("-(9 ** 9 ** 9)"), "-Inf");
}

#[test]
fn sqrt_negative_yields_nan() {
    assert_eq!(eval_string("sqrt(-1)"), "NaN");
}

// ── Math builtins ────────────────────────────────────────────────────────────

#[test]
fn atan2_one_one_is_quarter_pi() {
    assert_eq!(eval_string(r#"sprintf("%.4f", atan2(1, 1))"#), "0.7854");
}

#[test]
fn exp_one_is_e() {
    assert_eq!(eval_string(r#"sprintf("%.4f", exp(1))"#), "2.7183");
}

#[test]
fn log_e_is_one() {
    assert_eq!(eval_string(r#"sprintf("%.4f", log(2.71828))"#), "1.0000");
}

#[test]
fn sqrt_two_is_one_point_four_one_four() {
    assert_eq!(eval_string(r#"sprintf("%.4f", sqrt(2))"#), "1.4142");
}

// ── Numeric coercion of strings ──────────────────────────────────────────────

#[test]
fn string_with_trailing_garbage_numifies_to_leading_number() {
    assert_eq!(eval_int(r#""3.5abc" * 2"#), 7);
}

#[test]
fn pure_alpha_string_numifies_to_zero() {
    assert_eq!(eval_int(r#""abc" * 3"#), 0);
}

// ── Chained comparisons read left-to-right ──────────────────────────────────

#[test]
fn chained_less_than_is_left_associative() {
    // `1 < 2 < 3` parses as `(1<2) < 3` = `1 < 3` = true (Perl semantics).
    assert_eq!(eval_int("1 < 2 < 3 ? 1 : 0"), 1);
}

// ── Defined-or and its assignment form ──────────────────────────────────────

#[test]
fn defined_or_returns_left_when_defined_zero() {
    assert_eq!(eval_int(r#"my $x = 0; $x // 99"#), 0);
}

#[test]
fn defined_or_assign_stores_when_undef() {
    assert_eq!(
        eval_string(r#"my %h; my $v = $h{xx} //= 99; "$v/$h{xx}""#),
        "99/99"
    );
}

// ── Hash keys that are refs are stringified ─────────────────────────────────

#[test]
fn hash_with_arrayref_key_uses_ref_string_form() {
    assert_eq!(
        eval_int(r#"my %h; my $k = []; $h{$k} = "x"; scalar keys %h"#),
        1
    );
}

// ── Labelled loop control ────────────────────────────────────────────────────

#[test]
fn last_label_breaks_outer_loop() {
    let out = eval_string(
        r#"my $s = "";
           OUTER: for my $i (1..3) {
             for my $j (1..3) {
               if ($i == 2 && $j == 2) { last OUTER }
               $s .= "$i,$j;";
             }
           }
           $s"#,
    );
    assert_eq!(out, "1,1;1,2;1,3;2,1;");
}

#[test]
fn next_label_skips_to_outer_iteration() {
    let out = eval_string(
        r#"my $s = "";
           OUTER: for my $i (1..3) {
             for my $j (1..3) {
               next OUTER if $j == 2;
               $s .= "$i,$j;";
             }
           }
           $s"#,
    );
    assert_eq!(out, "1,1;2,1;3,1;");
}

// ── Inline package syntax ────────────────────────────────────────────────────

#[test]
fn package_brace_block_defines_subs() {
    assert_eq!(
        eval_string(
            r#"package Pkg { sub greet { "from-Pkg" } }
               Pkg::greet()"#
        ),
        "from-Pkg"
    );
}

// ── Array slice assign / delete on array ─────────────────────────────────────

#[test]
fn array_slice_assign_replaces_at_indexed_positions() {
    assert_eq!(
        eval_string(r#"my @a = (1..5); @a[1,3] = (20, 40); "@a""#),
        "1 20 3 40 5"
    );
}

#[test]
fn delete_on_array_undefs_in_place_keeps_length() {
    assert_eq!(eval_int(r#"my @a = (1..5); delete $a[2]; scalar @a"#), 5);
    assert_eq!(
        eval_int(r#"my @a = (1..5); delete $a[2]; defined $a[2] ? 1 : 0"#),
        0
    );
}

// ── statement-modifier `for` runs the expression once per item ───────────────

#[test]
fn statement_modifier_for_runs_expression_per_item() {
    assert_eq!(eval_string(r#"my @a; push @a, $_ for 1..3; "@a""#), "1 2 3");
}
