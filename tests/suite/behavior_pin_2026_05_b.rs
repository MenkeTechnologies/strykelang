//! Behavior-pinning batch B (2026-05-04): OOP, modules, parallel, file I/O,
//! sprintf edges, modulo sign, and pipe semantics.
//!
//! Companion to `behavior_pin_2026_05.rs`. Each block of tests pins observed
//! v0.11.x behavior; entries flagged in `docs/BUGS.md` are pinned to their
//! *current* (sometimes wrong) output and will need updating when fixed.

use crate::common::*;

// ── `bless` and ref stringification ──────────────────────────────────────────

#[test]
fn bless_hashref_ref_returns_class_name() {
    assert_eq!(
        eval_string(r#"my $o = bless {x=>1}, "Foo"; ref($o)"#),
        "Foo"
    );
}

#[test]
fn bless_arrayref_ref_returns_class_name() {
    assert_eq!(
        eval_string(r#"my $o = bless [1,2,3], "Bar"; ref($o)"#),
        "Bar"
    );
}

#[test]
fn bless_arrayref_stringifies_with_hash_tag_today() {
    // Documented in BUGS.md (BUG-002): stringification format ignores
    // underlying ref kind for blessed array refs.
    let s = eval_string(r#"my $o = bless [1,2,3], "Bar"; "$o""#);
    assert!(
        s.starts_with("Bar=HASH("),
        "expected current buggy `Bar=HASH(...)` form, got {:?}",
        s
    );
}

// ── Native `class { ... }` syntax ─────────────────────────────────────────────

#[test]
fn class_field_default_and_method() {
    assert_eq!(
        eval_int(
            r#"class Counter { value: Int = 0; fn bump { $self->value($self->value + 1); $self->value } }
               my $c = Counter(); $c->bump; $c->bump; $c->bump"#
        ),
        3
    );
}

#[test]
fn class_extends_overrides_parent_method() {
    assert_eq!(
        eval_string(
            r#"class Animal { fn speak { "generic" } }
               class Dog extends Animal { fn speak { "woof" } }
               Dog()->speak"#
        ),
        "woof"
    );
}

#[test]
fn class_extends_inherits_unimplemented_method() {
    assert_eq!(
        eval_string(
            r#"class Animal { fn speak { "generic" } }
               class Dog extends Animal { }
               Dog()->speak"#
        ),
        "generic"
    );
}

#[test]
fn class_chained_method_returns_self() {
    assert_eq!(
        eval_string(
            r#"class Builder {
                 s: Str = ""
                 fn add($x) { $self->s($self->s . $x); $self }
               }
               Builder()->add("a")->add("b")->add("c")->s"#
        ),
        "abc"
    );
}

// ── Perl-5-style packages with @ISA ───────────────────────────────────────────

#[test]
fn perl5_isa_inheritance_resolves_method() {
    assert_eq!(
        eval_string(
            r#"package Animal; sub new { bless {}, shift } sub speak { "generic" }
               package Dog; our @ISA = ("Animal"); sub speak { "woof" }
               package main;
               Dog->new->speak"#
        ),
        "woof"
    );
}

#[test]
fn perl5_super_call_through_isa_works() {
    // The Perl 5 `our @ISA = (...)` + `SUPER::` form works correctly.
    // Contrast with `class extends` + `$self->SUPER::method` which currently
    // stack-overflows (BUG-003).
    assert_eq!(
        eval_string(
            r#"package Animal; sub new { bless {}, shift } sub speak { "generic" }
               package Dog; our @ISA = ("Animal");
               sub speak { my $self = shift; "woof+" . $self->SUPER::speak }
               package main;
               Dog->new->speak"#
        ),
        "woof+generic"
    );
}

#[test]
fn perl5_multiple_inheritance_left_to_right() {
    assert_eq!(
        eval_string(
            r#"package A; sub a { "A" }
               package B; sub b { "B" }
               package C; our @ISA = ("A", "B");
               package main;
               C->a . "/" . C->b"#
        ),
        "A/B"
    );
}

#[test]
fn isa_method_returns_truth() {
    assert_eq!(
        eval_int(
            r#"package Dog; our @ISA = ("Animal");
               package main;
               Dog->isa("Animal") ? 1 : 0"#
        ),
        1
    );
    assert_eq!(
        eval_int(
            r#"package Dog; our @ISA = ("Animal");
               package main;
               Dog->isa("Cat") ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn destroy_runs_at_scope_exit() {
    // Pin: DESTROY fires when the only ref drops at scope end. Uses a
    // package-global (`our`) rather than a lexical so DESTROY's qualified
    // write reaches the same storage the test reads back.
    let out = eval_string(
        r#"our $log = "";
           package O; sub new { bless {}, shift } sub DESTROY { $main::log .= "bye:" }
           package main;
           { my $x = O->new; $log .= "have:"; }
           $log .= "after"; $log"#,
    );
    assert!(out.contains("have:"), "got {:?}", out);
    assert!(out.contains("bye:"), "DESTROY missing in {:?}", out);
    assert!(out.contains("after"), "got {:?}", out);
}

// ── Pipe operator `|>` semantics ─────────────────────────────────────────────

#[test]
fn pipe_with_array_var_through_map_and_sum() {
    assert_eq!(
        eval_int(r#"my @a = (1..5); sum(@a |> map { _ * 2 })"#),
        30
    );
}

#[test]
fn pipe_with_array_var_into_sum() {
    assert_eq!(eval_int(r#"my @a = (1..5); @a |> sum"#), 15);
}

#[test]
fn pipe_with_paren_list_through_map() {
    assert_eq!(
        eval_string(r#"my @r = (1..5) |> map { _ * 2 }; "@r""#),
        "2 4 6 8 10"
    );
}

#[test]
fn pipe_with_arrayref_into_sum_returns_zero_today() {
    // BUG-004: pipe with an arrayref LHS does not auto-deref.
    // `[1..5] |> sum` returns 0 instead of 15. Pinned at current value.
    assert_eq!(eval_int(r#"[1..5] |> sum"#), 0);
}

#[test]
fn pipe_with_arrayref_through_map_returns_single_zero_today() {
    // BUG-004 cont'd: `[1..5] |> map { _ * 2 }` does NOT auto-deref the
    // arrayref. `_` becomes the arrayref itself, which multiplied by 2 gives
    // 0 (numified ref). The map runs exactly once and the result is `(0)`.
    assert_eq!(
        eval_string(r#"my @r = [1..5] |> map { _ * 2 }; scalar(@r) . ":" . $r[0]"#),
        "1:0"
    );
}

// ── Parallel builtins ─────────────────────────────────────────────────────────

#[test]
fn pmap_over_range_doubles_values() {
    assert_eq!(
        eval_string(r#"my @r = pmap { _ * 2 } 1..5; "@r""#),
        "2 4 6 8 10"
    );
}

#[test]
fn pmap_over_range_dollar_underscore() {
    assert_eq!(
        eval_string(r#"my @r = pmap { $_ * 2 } 1..5; "@r""#),
        "2 4 6 8 10"
    );
}

#[test]
fn pgrep_filters_evens() {
    assert_eq!(
        eval_string(r#"my @r = pgrep { _ % 2 == 0 } 1..10; "@r""#),
        "2 4 6 8 10"
    );
}

#[test]
fn psort_orders_numbers_ascending() {
    assert_eq!(
        eval_string(r#"my @r = psort { $a <=> $b } 5,3,1,4,2; "@r""#),
        "1 2 3 4 5"
    );
}

// ── Async / await ─────────────────────────────────────────────────────────────

#[test]
fn async_await_returns_value() {
    assert_eq!(
        eval_int(r#"my $f = async { 42 }; await $f"#),
        42
    );
}

// ── `chomp` array form is a wart ──────────────────────────────────────────────
//
// Pinning current behavior; investigate later. Single-scalar chomp is fine.

#[test]
fn chomp_single_scalar_strips_newline_and_returns_count() {
    assert_eq!(
        eval_string(r#"my $s = "hi\n"; my $n = chomp($s); "n=$n s=[$s]""#),
        "n=1 s=[hi]"
    );
}

// ── Modulo follows Perl-style floored division (PARITY-005 FIXED) ───────────
//
// Result has the sign of the divisor (or is zero), matching Perl 5.

#[test]
fn mod_negative_dividend_positive_divisor_returns_positive() {
    assert_eq!(eval_int("-7 % 3"), 2);
}

#[test]
fn mod_positive_dividend_negative_divisor_returns_negative() {
    assert_eq!(eval_int("7 % -3"), -2);
}

#[test]
fn mod_negative_dividend_negative_divisor_returns_negative() {
    assert_eq!(eval_int("-7 % -3"), -1);
}

#[test]
fn mod_positive_positive_matches_perl() {
    assert_eq!(eval_int("7 % 3"), 1);
}

#[test]
fn mod_compound_assign_uses_floored_division() {
    assert_eq!(eval_int(r#"my $x = -7; $x %= 3; $x"#), 2);
    assert_eq!(eval_int(r#"my $x = 7; $x %= -3; $x"#), -2);
}

// ── sprintf format-specifier coverage ────────────────────────────────────────

#[test]
fn sprintf_g_format_picks_shortest_representation() {
    // PARITY-006 FIXED: %g picks the shorter of %f / %e and strips trailing
    // zeros, matching Perl's libc-style format.
    assert_eq!(eval_string(r#"sprintf("%g", 0.0001)"#), "0.0001");
    assert_eq!(eval_string(r#"sprintf("%g", 1234567)"#), "1.23457e+06");
    assert_eq!(eval_string(r#"sprintf("%g", 1.234567890123456)"#), "1.23457");
    assert_eq!(eval_string(r#"sprintf("%g", 0.00001)"#), "1e-05");
    assert_eq!(eval_string(r#"sprintf("%G", 1.234e-5)"#), "1.234E-05");
}

#[test]
fn sprintf_e_format_uses_perl_exponent_form() {
    // PARITY-007 FIXED: exponent has explicit sign and is zero-padded to 2.
    assert_eq!(
        eval_string(r#"sprintf("%e", 12345.6789)"#),
        "1.234568e+04"
    );
    assert_eq!(eval_string(r#"sprintf("%.0e", 12345)"#), "1e+04");
    assert_eq!(eval_string(r#"sprintf("%E", 12345.6789)"#), "1.234568E+04");
}

#[test]
fn sprintf_v_format_yields_dot_joined_byte_values() {
    // PARITY-008 FIXED: `%vd` formats each byte of the arg as a decimal,
    // joined by ".".
    assert_eq!(
        eval_string(r#"sprintf("%vd", "1.2.3")"#),
        "49.46.50.46.51"  // ASCII for '1','.','2','.','3'
    );
    assert_eq!(eval_string(r#"sprintf("%vd", "abc")"#), "97.98.99");
    assert_eq!(eval_string(r#"sprintf("%vx", "AB")"#), "41.42");
}

#[test]
fn sprintf_positional_arg_picks_arg_at_index() {
    // PARITY-009 FIXED: `%N$X` picks args[N-1] for the conversion's value
    // without advancing the sequential cursor.
    assert_eq!(
        eval_string(r#"sprintf("%2\$s %1\$s", "world", "hello")"#),
        "hello world"
    );
    // Same arg used twice.
    assert_eq!(
        eval_string(r#"sprintf("%1\$s-%1\$s", "echo")"#),
        "echo-echo"
    );
}

#[test]
fn sprintf_percent_literal() {
    assert_eq!(eval_string(r#"sprintf("100%% done")"#), "100% done");
}

#[test]
fn sprintf_undef_stringifies_to_empty() {
    assert_eq!(eval_string(r#"sprintf("[%s]", undef)"#), "[]");
}

// ── `caller(N)` returns subroutine name as undef today ────────────────────────
//
// BUG-005: `(caller(N))[3]` should be the fully-qualified subroutine name.
// stryke leaves it undef (joins to empty in the output).

#[test]
fn caller_zero_omits_subroutine_name_today() {
    let out = eval_string(
        r#"sub gx { my @c = caller(0); join(",", map { defined $_ ? $_ : "" } @c[0,1,3]) }
           sub fnx { gx() }
           fnx()"#,
    );
    // First two are package + filename, both populated. Fourth (sub name) is
    // currently empty — pin that.
    assert!(out.starts_with("main,-e,"), "unexpected prefix: {:?}", out);
    assert!(out.ends_with(","), "expected trailing empty subname, got {:?}", out);
}

// ── `kv-slice` yields key-value pairs (BUG-008 FIXED) ────────────────────────

#[test]
fn kv_slice_returns_subset_with_key_value_pairs() {
    // BUG-008 FIXED: `%h{KEYS}` is Perl 5.20+ key-value slice. Returns a
    // flat (key, value, key, value, ...) list, so assigning to `%sub`
    // produces a hash containing only the requested keys.
    let out = eval_string(
        r#"my %h = (a=>1, b=>2, c=>3);
           my %sub = %h{qw(a c)};
           join(",", map { "$_=$sub{$_}" } sort keys %sub)"#,
    );
    assert_eq!(out, "a=1,c=3");
}

#[test]
fn kv_slice_into_array_yields_alternating_key_value_pairs() {
    let out = eval_string(
        r#"my %h = (a=>1, b=>2, c=>3);
           my @kv = %h{qw(a c)};
           join(",", @kv)"#,
    );
    assert_eq!(out, "a,1,c,3");
}

// ── `exists` chain on missing intermediate hash ───────────────────────────────
//
// BUG-009 FIXED: `exists $h{x}{y}` when $h{x} is missing now returns false
// (was: erroring with "exists argument is not a HASH reference"). Multi-
// level chains soft-fail at any missing intermediate, matching Perl 5.

#[test]
fn exists_on_missing_intermediate_returns_false() {
    assert_eq!(
        eval_int(r#"my %h = (a => {b => 1}); exists $h{x}{y} ? 1 : 0"#),
        0
    );
}

#[test]
fn exists_on_present_chain_returns_true() {
    assert_eq!(
        eval_int(r#"my %h = (a => {b => 1}); exists $h{a}{b} ? 1 : 0"#),
        1
    );
}

#[test]
fn exists_on_three_level_missing_returns_false() {
    assert_eq!(
        eval_int(
            r#"my %h = (a => {b => {c => 1}}); exists $h{x}{y}{z} ? 1 : 0"#
        ),
        0
    );
    assert_eq!(
        eval_int(
            r#"my %h = (a => {b => {c => 1}}); exists $h{a}{b}{c} ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn exists_through_array_chain_soft_fails() {
    // exists $a[5][0] where @a only has indices 0..1 — Perl returns false
    // at the deepest test without erroring on the undef intermediate.
    assert_eq!(
        eval_int(r#"my @a = ([1,2,3], [4,5,6]); exists $a[5][0] ? 1 : 0"#),
        0
    );
    assert_eq!(
        eval_int(r#"my @a = ([1,2,3], [4,5,6]); exists $a[0][1] ? 1 : 0"#),
        1
    );
}

#[test]
fn exists_through_non_ref_intermediate_returns_false() {
    // $h{a} = 5 (scalar) — `exists $h{a}{x}` returns false in Perl.
    assert_eq!(
        eval_int(r#"my %h = (a => 5); exists $h{a}{x} ? 1 : 0"#),
        0
    );
}

// ── `Util->greet(...)` of `fn Self.greet($name)` passes class as $name ───────
//
// BUG-008: invoking a `Self.method` via `->` passes the class name into the
// first declared param instead of routing it as the implicit receiver.
// Pinned at current behavior.

#[test]
fn arrow_invoke_of_static_method_passes_class_as_first_arg_today() {
    assert_eq!(
        eval_string(
            r#"class Util { fn Self.greet($name) { "hi, $name" } }
               Util->greet("world")"#
        ),
        "hi, Util"
    );
}

// ── try / catch ───────────────────────────────────────────────────────────────

#[test]
fn try_catch_catches_die_string() {
    let out = eval_string(r#"my $e = ""; try { die "boom" } catch ($x) { $e = $x } $e"#);
    assert!(out.contains("boom"), "got {:?}", out);
}

#[test]
fn die_with_arrayref_preserves_ref_in_dollar_at() {
    assert_eq!(
        eval_int(r#"eval { die [10, 20, 30] }; scalar @{$@}"#),
        3
    );
    assert_eq!(
        eval_string(r#"eval { die [10, 20, 30] }; ref $@"#),
        "ARRAY"
    );
}

// ── Subroutine signatures (Perl 5.20+ style) ─────────────────────────────────

#[test]
fn use_feature_signatures_works() {
    assert_eq!(
        eval_int(r#"use feature "signatures"; sub addit ($x, $y) { $x + $y } addit(2, 3)"#),
        5
    );
}

#[test]
fn prototype_returns_string() {
    assert_eq!(
        eval_string(
            r#"sub myfx ($) { $_[0] } my $r = \&myfx; prototype($r)"#
        ),
        "$"
    );
}

// ── `localtime` shape ─────────────────────────────────────────────────────────

#[test]
fn localtime_list_returns_nine_fields() {
    assert_eq!(eval_int(r#"my @t = localtime 0; scalar @t"#), 9);
}

// ── Defined-or vs logical-or ──────────────────────────────────────────────────

#[test]
fn defined_or_preserves_zero() {
    assert_eq!(
        eval_string(r#"my $x = 0; "[" . ($x // "DEFAULT") . "]""#),
        "[0]"
    );
}

#[test]
fn logical_or_replaces_zero() {
    assert_eq!(
        eval_string(r#"my $x = 0; "[" . ($x || "DEFAULT") . "]""#),
        "[DEFAULT]"
    );
}

// ── `last` and `next` in for loops ────────────────────────────────────────────

#[test]
fn last_breaks_loop() {
    assert_eq!(
        eval_string(r#"my $s = ""; for (1..5) { last if $_ == 3; $s .= $_ } $s"#),
        "12"
    );
}

#[test]
fn next_skips_iteration() {
    assert_eq!(
        eval_string(r#"my $s = ""; for (1..5) { next if $_ % 2; $s .= $_ } $s"#),
        "24"
    );
}

// ── File-test operators ──────────────────────────────────────────────────────

#[test]
fn file_test_e_on_existing_dir() {
    assert_eq!(eval_int(r#"-e "/tmp" ? 1 : 0"#), 1);
}

#[test]
fn file_test_d_distinguishes_dir_from_file() {
    assert_eq!(eval_int(r#"-d "/tmp" ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"-f "/tmp" ? 1 : 0"#), 0);
}

#[test]
fn file_test_e_on_missing_path_is_false() {
    assert_eq!(eval_int(r#"-e "/no/such/path/xxxx" ? 1 : 0"#), 0);
}

// ── Hash slice (@h{...}) and delete-returns-value ────────────────────────────

#[test]
fn array_hash_slice_returns_values_in_key_order() {
    assert_eq!(
        eval_string(r#"my %h = (a=>1, b=>2, c=>3); my @v = @h{qw(a c)}; "@v""#),
        "1 3"
    );
}

#[test]
fn delete_returns_deleted_value() {
    assert_eq!(
        eval_int(r#"my %h = (a=>1, b=>2); my $x = delete $h{a}; $x"#),
        1
    );
}

// ── Regex named captures ──────────────────────────────────────────────────────

#[test]
fn regex_named_capture_via_plus_hash() {
    assert_eq!(
        eval_string(r#""hello" =~ /(?<word>\w+)/; $+{word}"#),
        "hello"
    );
}

#[test]
fn regex_two_named_captures() {
    assert_eq!(
        eval_string(r#""abc 123" =~ /(?<w>\w+)\s+(?<n>\d+)/; "$+{w}/$+{n}""#),
        "abc/123"
    );
}

// ── Pack / unpack roundtrip ──────────────────────────────────────────────────

#[test]
fn pack_n_and_N_roundtrip_through_hex() {
    assert_eq!(
        eval_string(r#"my $b = pack("nN", 1, 2); unpack("H*", $b)"#),
        "000100000002"
    );
}

// ── Underscore in numeric literals ────────────────────────────────────────────

#[test]
fn numeric_literal_with_underscores() {
    assert_eq!(eval_int("1_000_000"), 1_000_000);
}

// ── int() truncates toward zero ──────────────────────────────────────────────

#[test]
fn int_truncates_positive_toward_zero() {
    assert_eq!(eval_int("int(3.9)"), 3);
}

#[test]
fn int_truncates_negative_toward_zero() {
    assert_eq!(eval_int("int(-3.9)"), -3);
}

// ── Integer overflow and shift ───────────────────────────────────────────────

#[test]
fn left_shift_of_one_by_ten() {
    assert_eq!(eval_int("1 << 10"), 1024);
}

#[test]
fn right_shift_of_kilobyte_by_three() {
    assert_eq!(eval_int("1024 >> 3"), 128);
}

#[test]
fn complement_of_zero_is_minus_one() {
    assert_eq!(eval_int("~0"), -1);
}

// ── @ARGV preserved across run ───────────────────────────────────────────────

#[test]
fn lib_run_via_eval_does_not_populate_argv_implicitly() {
    // When evaluating via the library API, @ARGV is empty by default.
    assert_eq!(eval_int("scalar @ARGV"), 0);
}
