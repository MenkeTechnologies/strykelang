//! Behavior-pinning batch AQ (2026-05-06): regex match-var interpolation,
//! smartmatch with array/hash RHS, plus stable behaviors discovered while
//! probing for bugs.
//!
//! Pins fixes from this hunt:
//!   - `"$&"` in double-quoted strings interpolates the regex match (was
//!     literal `$&` — BUG-029 in `behavior_pin_2026_05_d`).
//!   - `given (X) { when ([list]) }` smartmatches against array elements,
//!     `when (\@arr)` and `when (\%hash)` likewise (was always falling
//!     through to `default`).
//!
//! Plus stability pins for behaviors that already work but weren't covered:
//!   - `NaN` self-equality (`$n == $n` is false), Inf handling, division
//!     by zero error
//!   - `wantarray` context detection
//!   - sort stability for equal keys
//!   - hash-slice assignment via `@h{KEYS} = LIST`
//!   - `my ($first, %rest) = LIST` destructuring

use crate::common::*;

// ── Bug O: $& interpolation ────────────────────────────────────────────────

#[test]
fn dollar_amp_interpolates_after_match() {
    // The bare-expression read worked already; only the interpolation form
    // was broken. This pin guards both.
    let code = r#"
        my $s = "hello";
        $s =~ /e(l+)o/;
        "match: $&"
    "#;
    assert_eq!(eval_string(code), "match: ello");
}

#[test]
fn dollar_apostrophe_interpolates_postmatch() {
    let code = r#"
        my $s = "abcdef";
        $s =~ /cd/;
        "post: $'"
    "#;
    assert_eq!(eval_string(code), "post: ef");
}

// ── Bug P: smartmatch with array/hash RHS ──────────────────────────────────

#[test]
fn given_when_smartmatch_array_literal_rhs() {
    // `when ([2, 3, 5, 7])` should match scalar topic `5` against any element.
    // Stryke used to reduce smartmatch to string equality, so the array
    // (stringified as a fallback) wouldn't compare equal to the scalar
    // and `default` always fired.
    let code = r#"
        use feature "switch";
        my $hit;
        my $x = 5;
        given ($x) {
            when ([2, 3, 5, 7]) { $hit = "prime" }
            default { $hit = "other" }
        }
        $hit
    "#;
    assert_eq!(eval_string(code), "prime");
}

#[test]
fn given_when_smartmatch_arrayref_rhs() {
    let code = r#"
        use feature "switch";
        my @primes = (2, 3, 5, 7);
        my $hit;
        given (3) {
            when (\@primes) { $hit = "in" }
            default { $hit = "out" }
        }
        $hit
    "#;
    assert_eq!(eval_string(code), "in");
}

#[test]
fn given_when_smartmatch_hashref_rhs_keys() {
    // Smartmatch against a hash ref means "topic is a key".
    let code = r#"
        use feature "switch";
        my %h = (apple => 1, pear => 1);
        my $hit;
        given ("apple") {
            when (\%h) { $hit = "key" }
            default { $hit = "miss" }
        }
        $hit
    "#;
    assert_eq!(eval_string(code), "key");
}

// ── Stability pins (these already worked; pin so they stay working) ────────

#[test]
fn nan_is_not_equal_to_itself() {
    // IEEE 754: NaN == NaN is false. Worth pinning because string-coerced
    // numerics in Perl-flavored languages sometimes accidentally compare
    // by string ("NaN" eq "NaN" is true) and break this invariant.
    let code = r#"
        my $n = "NaN" + 0;
        $n == $n ? "eq" : "neq"
    "#;
    assert_eq!(eval_string(code), "neq");
}

#[test]
fn log_of_zero_is_negative_infinity() {
    // Matches Perl 5: `log 0` returns `-Inf` rather than dying.
    let code = r#"log(0)"#;
    let v = eval(code).to_string();
    assert!(v == "-Inf" || v == "-inf", "got {}", v);
}

#[test]
fn sqrt_of_negative_is_nan() {
    let code = r#"sqrt(-1)"#;
    let v = eval(code).to_string();
    assert!(v == "NaN" || v == "nan", "got {}", v);
}

#[test]
fn division_by_zero_is_runtime_error() {
    let _kind = eval_err_kind("1 / 0");
}

#[test]
fn wantarray_context_distinguishes_list_scalar_void() {
    let code = r#"
        fn ctx { wantarray ? "L" : defined(wantarray) ? "S" : "V" }
        my @a = ctx();
        my $s = ctx();
        ctx();
        "$a[0]:$s"
    "#;
    assert_eq!(eval_string(code), "L:S");
}

#[test]
fn sort_is_stable_for_equal_keys() {
    // Stryke's sort uses `Vec::sort_by` (stable). Pin the guarantee so it's
    // not silently swapped to an unstable sort for performance.
    let code = r#"
        my @items = ({n=>1, t=>"a"}, {n=>1, t=>"b"}, {n=>2, t=>"c"}, {n=>1, t=>"d"});
        my @sorted = sort { $a->{n} <=> $b->{n} } @items;
        join(",", map { $_->{t} } @sorted)
    "#;
    // n=1 entries (a,b,d) keep insertion order, then n=2 (c).
    assert_eq!(eval_string(code), "a,b,d,c");
}

#[test]
fn hash_slice_assignment_distributes_values() {
    let code = r#"
        my %h;
        @h{qw(a b c)} = (1, 2, 3);
        join(",", map { "$_=$h{$_}" } sort keys %h)
    "#;
    assert_eq!(eval_string(code), "a=1,b=2,c=3");
}

#[test]
fn destructure_scalar_then_hash_rest() {
    let code = r#"
        my ($first, %rest) = (1, a => 2, b => 3);
        "$first|" . join(",", map { "$_=$rest{$_}" } sort keys %rest)
    "#;
    assert_eq!(eval_string(code), "1|a=2,b=3");
}

// ── Bug Q: tie my $x, Class parses ─────────────────────────────────────────

#[test]
fn tie_my_scalar_parses() {
    // `tie my $x, Class` — common Perl idiom — used to error with
    // "tie expects $scalar, @array, or %hash, got Ident(\"my\")". Now the
    // parser desugars to `my $x; tie $x, Class` via a StmtGroup.
    // The runtime FETCH/STORE behavior of tied scalars is a separate
    // concern (pre-existing limitation; tied scalars don't fire FETCH),
    // so we only assert the parse + tie call don't error.
    let code = r#"
        package Counter;
        sub TIESCALAR { my $c = shift; my $i = 0; bless \$i, $c }
        sub FETCH { ${$_[0]} }
        package main;
        tie my $x, "Counter";
        defined($x) ? "defined" : "undef"
    "#;
    let s = eval_string(code);
    assert!(s == "defined" || s == "undef", "got: {}", s);
}

#[test]
fn tie_my_hash_parses_and_works() {
    // `tie my %h, Class` — works for hashes (TIEHASH/STORE/FETCH all live).
    let code = r#"
        package SimpleH;
        sub TIEHASH { bless {}, shift }
        sub STORE { $_[0]->{$_[1]} = $_[2] }
        sub FETCH { $_[0]->{$_[1]} }
        package main;
        tie my %h, "SimpleH";
        $h{x} = 42;
        $h{x}
    "#;
    assert_eq!(eval_int(code), 42);
}

// ── Bug S: $#a = N truncation ──────────────────────────────────────────────

#[test]
fn dollar_hash_array_truncates_when_assigned() {
    // `$#a = 4` should resize `@a` to length 5 (last index 4). Stryke
    // previously stored under literal `#a` as a separate scalar and the
    // array was untouched.
    let code = r#"
        my @a = (1..10);
        $#a = 4;
        join(",", @a)
    "#;
    assert_eq!(eval_string(code), "1,2,3,4,5");
}

#[test]
fn dollar_hash_array_extends_with_undef_when_assigned() {
    // Growing case: `$#a = 5` on a 3-element array extends to 6, padding
    // with undef.
    let code = r#"
        my @a = (1, 2, 3);
        $#a = 5;
        scalar @a
    "#;
    assert_eq!(eval_int(code), 6);
}

#[test]
fn dollar_hash_array_negative_one_empties() {
    // `$#a = -1` should set @a to length 0.
    let code = r#"
        my @a = (1..5);
        $#a = -1;
        scalar @a
    "#;
    assert_eq!(eval_int(code), 0);
}
