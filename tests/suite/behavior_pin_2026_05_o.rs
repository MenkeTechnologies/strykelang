//! Behavior-pinning batch O (2026-05-04): use constant, Readonly absence,
//! warnings pragma, AOP intercept management, sub block prototype `&@`,
//! complex regex /m, math edges.

use crate::common::*;

// ── use constant ────────────────────────────────────────────────────────────

#[test]
fn use_constant_simple_scalar() {
    assert_eq!(eval_string(r#"use constant PI => 3.14159; PI"#), "3.14159");
}

#[test]
fn use_constant_arithmetic() {
    assert_eq!(eval_int(r#"use constant LIM => 5; LIM * 2"#), 10);
}

#[test]
fn use_constant_arrayref_holds_list() {
    // Array-of-strings constants must be wrapped in an arrayref.
    assert_eq!(
        eval_string(r#"use constant DAYS => [qw(mon tue wed)]; join(",", @{DAYS()})"#),
        "mon,tue,wed"
    );
}

#[test]
fn use_constant_paren_list_collapses_to_last_today() {
    // BUG-086: `use constant ARR => (1, 2, 3)` should bind ARR to the list
    // (Perl does). Stryke binds it to the last comma operand only — same
    // root issue as BUG-010.
    assert_eq!(
        eval_string(r#"use constant ARR => (1, 2, 3); my @a = ARR; "@a""#),
        "3"
    );
}

#[test]
fn use_constant_hashref_form_is_rejected_today() {
    // BUG-086b: Perl supports `use constant { K1 => V1, K2 => V2, ... }`.
    // Stryke parses it as expecting a paired list, not a hashref.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"use constant { ZERO => 0, ONE => 1 }; ZERO"#);
    assert!(
        matches!(
            kind,
            ErrorKind::Runtime | ErrorKind::Type | ErrorKind::Syntax
        ),
        "expected error, got {:?}",
        kind
    );
}

// ── Readonly module not bundled today ──────────────────────────────────────

#[test]
fn readonly_module_not_loadable_today() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"use Readonly; Readonly my $X => 42"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::FileNotFound),
        "expected module-load error, got {:?}",
        kind
    );
}

#[test]
fn readonly_bareword_without_use_silently_returns_value() {
    // From the lib `eval` API, `Readonly my $X => 42` doesn't error — it
    // appears to return 42 as if `Readonly` were a name-able no-op. CLI
    // catches it as undefined sub. Pin lib behavior.
    assert_eq!(eval_int(r#"Readonly my $X => 42"#), 42);
}

// ── `use warnings` is parseable but doesn't emit warnings today ────────────

#[test]
fn use_warnings_silent_on_undef_arithmetic_today() {
    // BUG-087: Perl with `use warnings` emits "Use of uninitialized value"
    // for `$undef + 1`. Stryke runs silently. Pin: the program returns 1
    // (undef → 0 numerically) without diagnostics.
    assert_eq!(eval_int(r#"use warnings; my $x; my $y = $x + 1; $y"#), 1);
}

#[test]
fn use_warnings_silent_on_string_in_numeric_today() {
    assert_eq!(eval_int(r#"use warnings; "abc" + 1"#), 1);
}

#[test]
fn no_warnings_pragma_runs_without_error() {
    assert_eq!(eval_int(r#"no warnings; my $x; my $y = $x + 1; $y"#), 1);
}

// ── Math edge values ────────────────────────────────────────────────────────

#[test]
fn log_zero_is_negative_infinity() {
    assert_eq!(eval_string("log(0)"), "-Inf");
}

#[test]
fn log_negative_one_is_nan() {
    assert_eq!(eval_string("log(-1)"), "NaN");
}

#[test]
fn zero_to_zero_is_one() {
    assert_eq!(eval_int("0 ** 0"), 1);
}

#[test]
fn zero_to_negative_one_is_inf() {
    assert_eq!(eval_string("0 ** -1"), "Inf");
}

#[test]
fn sqrt_zero_one_four() {
    assert_eq!(
        eval_string(r#"join("/", sqrt(0), sqrt(1), sqrt(4))"#),
        "0/1/2"
    );
}

// ── String repeat with large counts ────────────────────────────────────────

#[test]
fn string_x_one_thousand_yields_thousand_chars() {
    assert_eq!(eval_int(r#"length("a" x 1000)"#), 1000);
}

#[test]
fn list_x_one_hundred_yields_hundred_elements() {
    assert_eq!(eval_int(r#"my @a = (0) x 100; scalar @a"#), 100);
}

// ── AOP intercept_clear / intercept_remove ─────────────────────────────────

#[test]
fn intercept_clear_removes_all_advice_for_target() {
    let out = eval_string(
        r#"our $log = "";
           fn payload { $main::log .= "G:" }
           before "payload" { $main::log .= "B:" }
           payload();              # B:G:
           intercept_clear("payload");
           payload();              # G:
           $log"#,
    );
    assert_eq!(out, "B:G:G:");
}

#[test]
fn intercept_remove_does_not_remove_advice_today() {
    // BUG-093: `intercept_remove("payload", "before")` is supposed to detach
    // just the "before" advice on `payload`, leaving "after" intact. Stryke
    // currently leaves all advice in place.
    let out = eval_string(
        r#"our $log = "";
           fn payload { $main::log .= "G:" }
           before "payload" { $main::log .= "B:" }
           after  "payload" { $main::log .= "A:" }
           payload();              # B:G:A:
           intercept_remove("payload", "before");
           payload();              # still B:G:A: today
           $log"#,
    );
    assert_eq!(out, "B:G:A:B:G:A:");
}

#[test]
fn intercept_list_returns_arrayref_in_list_context() {
    // `intercept_list()` returns one arrayref per registered intercept.
    let out = eval_string(
        r#"fn payload { 1 }
           before "payload" { 1 }
           my @l = intercept_list();
           ref($l[0])"#,
    );
    assert_eq!(out, "ARRAY");
}

// ── sub block prototype `(&)` works for one-arg-block form ─────────────────

#[test]
fn block_prototype_passes_block_as_first_arg() {
    assert_eq!(
        eval_int(r#"sub myff (&) { my $cb = shift; $cb->() } myff { 42 }"#),
        42
    );
}

// ── `(&@)` prototype drops trailing args today ─────────────────────────────

#[test]
fn block_at_prototype_with_trailing_args_evaluates_trailing_as_statements_today() {
    // BUG-088: `myff { ... } 5, 7` should call myff with @_ = (block, 5, 7).
    // Stryke parses `5, 7` as separate top-level comma operands AFTER the
    // myff call, so the script's overall value is 7 (the last operand) and
    // myff itself sees @_ = (block,) — i.e., scalar(@_) is 0 after shift.
    assert_eq!(
        eval_int(
            r#"sub myff (&@) { my $cb = shift; scalar @_ }
               myff { 1 } 5, 7"#
        ),
        7
    );
}

#[test]
fn coderef_call_with_named_array_arg_passes_through() {
    // Was BUG-037-related: passing `@args` to a captured `$cb` ref from
    // inside a sub body. After BUG-090 (`my ($cb, @args) = @_` slurpy
    // destructure) was fixed, `@args = (5)` and `$cb->(@args)` now
    // delivers $_[0]=5 to the coderef → 5 * 2 = 10.
    assert_eq!(
        eval_int(
            r#"sub myff { my ($cb, @args) = @_; $cb->(@args) }
               myff(sub { ($_[0] // 0) * 2 }, 5)"#
        ),
        10
    );
}

// ── Multiline regex /gm walks all line-anchored matches ────────────────────

#[test]
fn multiline_g_m_walks_all_kv_pairs() {
    let out = eval_string(
        r#"my $s = "k1=v1\nk2=v2\nk3=v3";
           my $log = "";
           while ($s =~ /^(\w+)=(\w+)$/gm) { $log .= "$1->$2;" }
           $log"#,
    );
    assert_eq!(out, "k1->v1;k2->v2;k3->v3;");
}

#[test]
fn substitution_with_m_flag_per_line() {
    assert_eq!(
        eval_string(r#"my $s = "abc\ndef\nghi"; $s =~ s/^(\w+)$/<$1>/gm; $s"#),
        "<abc>\n<def>\n<ghi>"
    );
}

// ── Hash autovivification on read does not create keys ─────────────────────

#[test]
fn hash_read_does_not_autoviv_top_level() {
    assert_eq!(
        eval_int(
            r#"my %h; my $v = $h{nonexistent_xx};
               (exists $h{nonexistent_xx}) ? 1 : 0"#
        ),
        0
    );
}

// ── Negative-zero printf format ────────────────────────────────────────────

#[test]
fn negative_zero_via_g_format_keeps_sign() {
    let s = eval_string(r#"sprintf("%g", -0.0)"#);
    assert!(s.starts_with("-"), "expected leading minus, got {:?}", s);
}

#[test]
fn negative_zero_compares_equal_to_positive() {
    assert_eq!(eval_int(r#"-0.0 == 0.0 ? 1 : 0"#), 1);
}

// ── Integer overflow vs i64::MIN ───────────────────────────────────────────

#[test]
fn i64_min_literal_via_subtraction_wraps_to_i64_max() {
    // `-9223372036854775808` cannot be parsed as a literal because the
    // positive form doesn't fit in i64. Build i64::MIN by subtraction and
    // confirm the wrap.
    assert_eq!(eval_int("(-9223372036854775807) - 1 - 1"), i64::MAX);
}

// ── Complex regex with captured groups vs /m anchor ────────────────────────

#[test]
fn matching_per_line_returns_all_captured_lines() {
    assert_eq!(
        eval_string(
            r#"my $s = "alpha\nbeta\ngamma";
               my @l = ($s =~ /^(\w+)$/mg);
               "@l""#
        ),
        "alpha beta gamma"
    );
}

// ── `qw()` in a constant becomes an arrayref tag today ─────────────────────

#[test]
fn use_constant_qw_becomes_arrayref_string() {
    // `use constant DAYS => qw(mon tue wed)` does not bind to a list of
    // three strings — instead, DAYS becomes a single value that
    // stringifies as `ARRAY(0x...)` (i.e., an arrayref wrapping the qw
    // list).
    let s = eval_string(r#"use constant DAYS => qw(mon tue wed); my $x = DAYS; "$x""#);
    assert!(
        s.starts_with("ARRAY("),
        "expected ARRAY(...) form, got {:?}",
        s
    );
}

// ── --w / -W CLI flags are accepted (parse-level pin only) ─────────────────
//
// Cannot drive `-w`/`-W` from inside `eval_string`; pin that the underlying
// programs they enable still parse. (BUG-087 covers their no-op behavior.)

#[test]
fn lib_eval_runs_undef_arith_without_warnings() {
    // Pinned at the actual numeric result (1) — this would warn under Perl
    // -w, but stryke produces no diagnostic.
    assert_eq!(eval_int(r#"my $x; my $y = $x + 1; $y"#), 1);
}

// ── Use constant computed at compile time ──────────────────────────────────

#[test]
fn use_constant_evaluated_at_definition_time() {
    // The RHS of `use constant` is captured at definition; later
    // mutations to source values don't affect the constant.
    assert_eq!(
        eval_int(r#"my $base = 5; use constant FIVE => 5; $base = 999; FIVE"#),
        5
    );
}

// ── Explicit-paren call form for `(&@)` ────────────────────────────────────

#[test]
fn destructuring_my_scalar_array_takes_at_underscore_tail() {
    // BUG-090 / BUG-095 FIXED: `my ($cb, @rest) = @_` binds $cb to the
    // first element and @rest to the tail (5, 7) — not the full @_.
    let out = eval_string(
        r#"sub myff { my ($cb, @rest) = @_; "@rest/" . scalar(@rest) }
           myff(sub { 1 }, 5, 7)"#,
    );
    assert_eq!(out, "5 7/2");
}

// ── intercept_remove on missing kind no-ops ────────────────────────────────

#[test]
fn intercept_remove_unknown_kind_does_not_panic() {
    // Verify the runtime handles bogus advice kinds gracefully.
    let out = eval_string(
        r#"fn payload { 1 }
           before "payload" { 1 }
           intercept_remove("payload", "nonexistent_kind");
           "ok""#,
    );
    assert_eq!(out, "ok");
}

// ── Math const round-trip ──────────────────────────────────────────────────

#[test]
fn pi_squared_is_close_to_known_value() {
    assert_eq!(eval_string(r#"sprintf("%.4f", pi * pi)"#), "9.8696");
}

// ── String ranges with mixed chars ─────────────────────────────────────────

#[test]
fn string_range_letters_and_digits_advance_lex() {
    // "a"..."d" steps through lex chars.
    assert_eq!(eval_string(r#"join(",", "a"..."d")"#), "a,b,c,d");
}

// ── Array slice with stride via grep ────────────────────────────────────────

#[test]
fn every_other_element_via_grep_index() {
    assert_eq!(
        eval_string(
            r#"my @a = (1..10);
               my @e = @a[grep { $_ % 2 == 0 } (0..$#a)];
               "@e""#
        ),
        "1 3 5 7 9"
    );
}

// ── `printf` with too many format specs vs args ─────────────────────────────

#[test]
fn sprintf_with_extra_format_pads_zeros() {
    // Already pinned in batch H but kept here as a regression guard for
    // the format error mode.
    assert_eq!(eval_string(r#"sprintf("%d %d", 1)"#), "1 0");
}

// ── die rethrow chain with three levels ────────────────────────────────────

#[test]
fn three_level_die_rethrow_drops_innermost_log_today() {
    // BUG-094: with three nested `eval { die ... }` levels, the innermost
    // level's `$log .= "L1:" . $@` mutation is lost. The outer two
    // mutations (L2, L3) persist.
    let out = eval_string(
        r#"my $log = "";
           eval {
             eval {
               eval { die "innermost\n" };
               $log .= "L1:" . $@;
               die $@;
             };
             $log .= "L2:" . $@;
             die $@;
           };
           $log .= "L3:" . $@;
           $log"#,
    );
    assert_eq!(out, "L2:innermost\nL3:innermost\n");
}

// ── Array of hashrefs sorted by nested field ───────────────────────────────

#[test]
fn array_of_hashrefs_sort_by_two_fields() {
    assert_eq!(
        eval_string(
            r#"my @items = ({a=>1,b=>2}, {a=>1,b=>1}, {a=>0,b=>5});
               my @s = sort { $a->{a} <=> $b->{a} || $a->{b} <=> $b->{b} } @items;
               join(";", map { "$_->{a}/$_->{b}" } @s)"#
        ),
        "0/5;1/1;1/2"
    );
}

// ── Ref to ref via `\\` ─────────────────────────────────────────────────────

#[test]
fn ref_to_ref_double_dereference_via_two_step() {
    // `${$$rr}[0]` doesn't work the same as `$$rr->[0]` in stryke. Use the
    // two-step form which does.
    assert_eq!(
        eval_int(
            r#"my @a = (10, 20); my $r = \@a; my $rr = \$r;
               my $r2 = $$rr; $r2->[0]"#
        ),
        10
    );
}

// ── Deeply nested data via JSON round-trip ─────────────────────────────────

#[test]
fn json_roundtrip_deep_structure() {
    assert_eq!(
        eval_int(
            r#"my $orig = { users => [{name=>"a", id=>1}, {name=>"b", id=>2}] };
               my $j = to_json($orig);
               my $back = from_json($j);
               $back->{users}[1]{id}"#
        ),
        2
    );
}
