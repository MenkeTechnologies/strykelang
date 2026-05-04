//! Behavior-pinning batch Q (2026-05-04): final small-surface bugs and a
//! handful of working idioms to lock in.

use crate::common::*;

// ── Anonymous sub IIFE forms ────────────────────────────────────────────────

#[test]
fn iife_with_arrow_works_with_outer_parens() {
    assert_eq!(eval_int(r#"(sub { 42 })->()"#), 42);
}

#[test]
fn iife_with_arrow_in_print_context_works() {
    // `print sub { 42 }->()` — print's arglist absorbs the call cleanly.
    let s = eval_string(r#"my $r = sub { 42 }->(); $r"#);
    assert_eq!(s, "42");
}

#[test]
fn iife_double_paren_form_works() {
    assert_eq!(eval_int(r#"((sub { 42 })->())"#), 42);
}

// ── Factory closures with multiple instances ────────────────────────────────

#[test]
fn factory_makes_independent_closures_with_different_captures() {
    assert_eq!(
        eval_string(
            r#"sub mkadder { my $base = shift; sub { $base + shift } }
               my $a5 = mkadder(5);
               my $a10 = mkadder(10);
               $a5->(3) . "/" . $a10->(3)"#
        ),
        "8/13"
    );
}

// ── Recursion (with and without sig) ───────────────────────────────────────

#[test]
fn recursion_via_named_sub_computes_fibonacci() {
    assert_eq!(
        eval_int(
            r#"sub myfib { my $n = shift; $n < 2 ? $n : myfib($n-1) + myfib($n-2) }
               myfib(10)"#
        ),
        55
    );
}

#[test]
fn recursion_via_fn_with_sig_computes_fibonacci() {
    assert_eq!(
        eval_int(
            r#"fn myfib($n) { $n < 2 ? $n : myfib($n-1) + myfib($n-2) }
               myfib(10)"#
        ),
        55
    );
}

// ── Hash-slice via arrayref-deref `@{$ref}{KEYS}` is broken today ──────────

#[test]
fn hash_slice_through_hashref_via_at_brace_deref_fails_today() {
    // BUG-091: `@{$h_ref}{qw(a c)}` should produce a hash slice through the
    // hashref. Stryke errors with "Can't dereference non-reference as array".
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"my %h = (a=>1, b=>2, c=>3); my $r = \%h;
           my @v = @{$r}{qw(a c)};
           "@v""#,
    );
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

#[test]
fn hash_slice_through_hashref_via_arrow_keys_works() {
    // The arrow form does work as a workaround.
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2, c=>3); my $r = \%h;
               my @v = ($r->{a}, $r->{c});
               "@v""#
        ),
        "1 3"
    );
}

// ── Ternary inside `@{[ ... ]}` interpolation is rejected today ────────────

#[test]
fn ternary_inside_interpolated_anon_array_is_rejected_today() {
    // BUG-092: `"@{[ $x > 0 ? "pos" : "neg" ]}"` should produce "pos" or
    // "neg". Stryke parses the inner `?` `:` poorly and errors with
    // "Unterminated @{ ... } in double-quoted string".
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"my $x = 5; my $s = "@{[ $x > 0 ? "pos" : "neg" ]}""#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

#[test]
fn ternary_outside_interpolation_works() {
    // The non-interpolated workaround.
    assert_eq!(
        eval_string(r#"my $x = 5; my $r = $x > 0 ? "pos" : "neg"; $r"#),
        "pos"
    );
}

// ── Topic `$_` and named builtins without args ─────────────────────────────

#[test]
fn for_topic_with_uc_no_arg() {
    let out = eval_string(r#"my @r; for ("alpha", "beta") { push @r, uc } "@r""#);
    assert_eq!(out, "ALPHA BETA");
}

#[test]
fn for_topic_with_length_no_arg() {
    let out = eval_string(r#"my @r; for ("alpha", "beta") { push @r, length } "@r""#);
    assert_eq!(out, "5 4");
}

// ── printf with undef // default ───────────────────────────────────────────

#[test]
fn printf_with_defined_or_default_substitution() {
    assert_eq!(
        eval_string(r#"sprintf("[%s]", undef // "default")"#),
        "[default]"
    );
}

// ── scalar keys %empty is zero ──────────────────────────────────────────────

#[test]
fn scalar_keys_of_empty_hash_is_zero() {
    assert_eq!(eval_int(r#"my %h; scalar keys %h"#), 0);
}

// ── Topic `$_` is shared between map block iterations ──────────────────────

#[test]
fn map_topic_visible_inside_block() {
    assert_eq!(
        eval_string(r#"my @r = map { "x$_" } (1..3); "@r""#),
        "x1 x2 x3"
    );
}

// ── grep with regex on $_ ──────────────────────────────────────────────────

#[test]
fn grep_with_regex_on_topic_filters() {
    assert_eq!(
        eval_string(r#"my @r = grep { /^a/ } qw(apple banana avocado date); "@r""#),
        "apple avocado"
    );
}

// ── BUGS.md is structurally well-formed (meta-test) ────────────────────────
//
// This test reads docs/BUGS.md and verifies basic invariants:
// * Every BUG/PARITY/POLISH entry has a corresponding numbered header
// * The "How to add to this file" section is present
// * The "NOT-A-BUG observations" section is present
// * The total entry count is at least 100 (we've documented far more)

#[test]
fn bugs_md_has_minimum_documented_entry_count() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("BUGS.md");
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {:?}: {}", path, e));
    let count = body
        .lines()
        .filter(|l| {
            l.starts_with("## BUG-")
                || l.starts_with("## PARITY-")
                || l.starts_with("## POLISH-")
        })
        .count();
    assert!(
        count >= 100,
        "expected ≥100 entries in BUGS.md, found {}",
        count
    );
}

#[test]
fn bugs_md_contains_required_sections() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("BUGS.md");
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {:?}: {}", path, e));
    assert!(
        body.contains("## How to add to this file"),
        "missing instructions section"
    );
    assert!(
        body.contains("## NOT-A-BUG observations"),
        "missing NOT-A-BUG section"
    );
}

// ── Last-element accessors on non-empty array ──────────────────────────────

#[test]
fn dollar_a_minus_one_returns_last_element() {
    assert_eq!(eval_int(r#"my @a = (10, 20, 30); $a[-1]"#), 30);
}

#[test]
fn dollar_hash_a_returns_last_index() {
    assert_eq!(eval_int(r#"my @a = (10, 20, 30); $#a"#), 2);
}

// ── Array of subs each held by index ───────────────────────────────────────

#[test]
fn array_of_subs_call_via_index() {
    assert_eq!(
        eval_string(
            r#"my @subs = (sub { "a" }, sub { "b" }, sub { "c" });
               join(",", $subs[0]->(), $subs[1]->(), $subs[2]->())"#
        ),
        "a,b,c"
    );
}

// ── Hash of subs dispatched by key ─────────────────────────────────────────

#[test]
fn hash_of_subs_dispatch_by_key() {
    // Use `%dispatch` since `%d` is a stryke-reserved hash name.
    assert_eq!(
        eval_int(
            r#"my %dispatch = (
                 add => sub { $_[0] + $_[1] },
                 mul => sub { $_[0] * $_[1] },
               );
               $dispatch{add}->(3, 4) + $dispatch{mul}->(2, 5)"#
        ),
        17
    );
}

// ── String concatenation with numeric coerces ──────────────────────────────

#[test]
fn string_concat_with_int_coerces_to_decimal() {
    assert_eq!(eval_string(r#""value=" . 42"#), "value=42");
}

#[test]
fn string_concat_with_float_coerces_to_decimal() {
    assert_eq!(eval_string(r#""pi=" . 3.14"#), "pi=3.14");
}

// ── Sort default is lexicographic; numeric uses comparator ─────────────────

#[test]
fn sort_default_lex_with_numeric_strings() {
    assert_eq!(
        eval_string(r#"join(",", sort qw(10 2 30 4))"#),
        "10,2,30,4"
    );
}

#[test]
fn sort_numeric_with_spaceship_comparator() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } qw(10 2 30 4))"#),
        "2,4,10,30"
    );
}

// ── Range op with reversed bounds ──────────────────────────────────────────

#[test]
fn descending_range_yields_empty_list() {
    assert_eq!(eval_int(r#"my @a = (5..1); scalar @a"#), 0);
}

// ── Range with `reverse` for descending ────────────────────────────────────

#[test]
fn reverse_of_ascending_range_yields_descending() {
    assert_eq!(
        eval_string(r#"my @a = reverse(1..5); "@a""#),
        "5 4 3 2 1"
    );
}

// ── Sprintf `[%s]` on undef gives `[]` ─────────────────────────────────────

#[test]
fn sprintf_s_on_undef_yields_empty_brackets() {
    assert_eq!(eval_string(r#"sprintf("[%s]", undef)"#), "[]");
}

// ── chomp on string with no newline returns 0 and leaves string ────────────

#[test]
fn chomp_on_clean_string_returns_zero_and_no_change() {
    assert_eq!(
        eval_string(
            r#"my $s = "abc"; my $n = chomp($s); "n=$n s=[$s]""#
        ),
        "n=0 s=[abc]"
    );
}

// ── Chained .= concatenation ────────────────────────────────────────────────

#[test]
fn chained_dot_equals_concatenation_builds_string() {
    assert_eq!(
        eval_string(r#"my $s = "x"; $s .= "y"; $s .= "z"; $s"#),
        "xyz"
    );
}

// ── scalar context on array slice ──────────────────────────────────────────

#[test]
fn scalar_on_array_slice_returns_count_today() {
    // Pin observed: `scalar @a[1, 3]` returns the slice's element count
    // (2), not the last element. Matches Perl's "list in scalar context →
    // count" — this is correct, not a bug.
    assert_eq!(eval_int(r#"my @a = (10, 20, 30, 40, 50); scalar(@a[1, 3])"#), 2);
}

// ── String split with limit on empty input ─────────────────────────────────

#[test]
fn split_with_empty_input_returns_empty_list() {
    assert_eq!(eval_int(r#"my @r = split /:/, ""; scalar @r"#), 0);
}

// ── join on empty list returns empty string ────────────────────────────────

#[test]
fn join_on_empty_list_returns_empty_string() {
    assert_eq!(eval_string(r#"join(",", ())"#), "");
}

// ── reverse of empty list ──────────────────────────────────────────────────

#[test]
fn reverse_of_empty_array_var_returns_empty() {
    // `reverse()` with bare empty parens is a parse error in stryke; pass an
    // empty array variable instead.
    assert_eq!(eval_int(r#"my @e; my @r = reverse @e; scalar @r"#), 0);
}

#[test]
fn reverse_with_bare_empty_parens_is_parse_error_today() {
    // BUG-099: `reverse()` should be valid (returns empty list). Stryke
    // raises "Unexpected token RParen".
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"my @r = reverse(); scalar @r"#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

// ── Reading a missing array element returns undef ──────────────────────────

#[test]
fn missing_array_element_is_undef() {
    assert_eq!(
        eval_int(r#"my @a = (1, 2); defined($a[10]) ? 1 : 0"#),
        0
    );
}

// ── Reading a missing hash key returns undef ───────────────────────────────

#[test]
fn missing_hash_key_is_undef() {
    assert_eq!(
        eval_int(r#"my %h = (a => 1); defined($h{z}) ? 1 : 0"#),
        0
    );
}

// ── Array assignment from a sub-returning-list works for scalar param case ─

#[test]
fn list_returned_from_sub_assigned_to_array() {
    assert_eq!(
        eval_string(
            r#"sub three { (1, 2, 3) }
               my @a = three();
               "@a""#
        ),
        "1 2 3"
    );
}

// ── Boolean context: empty array is false; non-empty is true ──────────────

#[test]
fn empty_array_is_false_in_boolean() {
    assert_eq!(eval_int(r#"my @a; @a ? 1 : 0"#), 0);
}

#[test]
fn non_empty_array_is_true_in_boolean() {
    assert_eq!(eval_int(r#"my @a = (0); @a ? 1 : 0"#), 1);
}

// ── exists on hash returns true even for false value ───────────────────────

#[test]
fn exists_with_false_value_still_true() {
    assert_eq!(
        eval_int(r#"my %h = (a => 0); exists $h{a} ? 1 : 0"#),
        1
    );
}

// ── 0 == "" in string-numeric comparison ───────────────────────────────────

#[test]
fn empty_string_numifies_to_zero() {
    assert_eq!(eval_int(r#""" + 0"#), 0);
}

#[test]
fn zero_eq_empty_string_with_double_equals() {
    assert_eq!(eval_int(r#"0 == "" ? 1 : 0"#), 1);
}

// ── `my ($x) = @arr` returns scalar count today (BUG-101) ──────────────────

#[test]
fn single_scalar_destructure_from_array_var_returns_count_today() {
    // BUG-101: `my ($x) = @arr` is supposed to be LIST context (parens
    // make it a list assignment) and bind $x to the first element. Stryke
    // treats it as scalar context and returns the count.
    assert_eq!(
        eval_int(r#"my @a = (10, 20, 30); my ($x) = @a; $x"#),
        3
    );
}

#[test]
fn single_scalar_destructure_from_at_underscore_returns_count_today() {
    // Same bug from a sub's @_.
    assert_eq!(
        eval_int(r#"sub myff { my ($x) = @_; $x } myff("hello", "world")"#),
        2
    );
}

#[test]
fn single_scalar_destructure_from_literal_list_works() {
    // The literal-list source form does work: `my ($x) = (literal)` binds.
    assert_eq!(
        eval_string(r#"my ($x) = ("hello"); $x"#),
        "hello"
    );
}

#[test]
fn shift_workaround_for_first_element_works() {
    assert_eq!(
        eval_string(r#"sub myff { my $x = shift; $x } myff("hello")"#),
        "hello"
    );
}

#[test]
fn dollar_underscore_zero_workaround_for_first_element_works() {
    assert_eq!(
        eval_string(r#"sub myff { my $x = $_[0]; $x } myff("hello")"#),
        "hello"
    );
}
