//! Python-style colon slice syntax: `@arr[FROM:TO:STEP]`, `@arr[::-1]`, `@h{a:z:1}`.
//!
//! Open-ended forms (`:N`, `N:`, `::`, `::-1`, `::M`) are intercepted by the parser
//! into `ExprKind::SliceRange`; the compiler emits `Op::ArraySliceRange` /
//! `Op::HashSliceRange` for any single-arg slice subscript that contains a colon
//! range (open OR closed) so the strict typing rules apply uniformly:
//!
//! - **Array slice** (`@arr[...]`): integer-strict — non-numeric strings, fractional
//!   floats, and refs as endpoints abort at runtime. Negative indices count from end.
//!   Both ends inclusive (matches Perl `..`).
//! - **Hash slice** (`@h{...}`): endpoints stringify to keys; barewords auto-quote
//!   (fat-comma style). Open-ended forms abort (no notion of "all keys" in unordered
//!   hash). Numeric ranges (`{1:3}`) work as string keys "1","2","3".

use crate::common::{eval_err_kind, eval_int, eval_string};
use stryke::error::ErrorKind;

// ── Closed integer ranges (existing behavior, now goes through ArraySliceRange) ──

#[test]
fn array_slice_closed_inclusive() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[1:3])"#),
        "20,30,40"
    );
}

#[test]
fn array_slice_closed_with_step() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[0:4:2])"#),
        "10,30,50"
    );
}

#[test]
fn array_slice_closed_negative_step() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[4:0:-1])"#),
        "50,40,30,20,10"
    );
}

// ── Open-ended forms ──

#[test]
fn array_slice_full_reversed_double_colon_neg_one() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[::-1])"#),
        "50,40,30,20,10"
    );
}

#[test]
fn array_slice_full_double_colon() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[::])"#),
        "10,20,30,40,50"
    );
}

#[test]
fn array_slice_open_start() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[:3])"#),
        "10,20,30,40"
    );
}

#[test]
fn array_slice_open_stop() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[2:])"#),
        "30,40,50"
    );
}

#[test]
fn array_slice_step_only() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[::2])"#),
        "10,30,50"
    );
}

#[test]
fn array_slice_negative_step_only() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[::-2])"#),
        "50,30,10"
    );
}

#[test]
fn array_slice_negative_index_open_stop() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[-3:])"#),
        "30,40,50"
    );
}

#[test]
fn array_slice_negative_index_open_start() {
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[:-2])"#),
        "10,20,30,40"
    );
}

// ── Integer-strict guard ──

#[test]
fn array_slice_string_endpoint_aborts() {
    assert_eq!(
        eval_err_kind(r#"my @a=(10,20,30); my @s = @a["a":"c"]; 0"#),
        ErrorKind::Runtime
    );
}

#[test]
fn array_slice_float_endpoint_aborts() {
    assert_eq!(
        eval_err_kind(r#"my @a=(10,20,30); my @s = @a[1.5:2]; 0"#),
        ErrorKind::Runtime
    );
}

#[test]
fn array_slice_zero_step_aborts() {
    assert_eq!(
        eval_err_kind(r#"my @a=(10,20,30); my @s = @a[::0]; 0"#),
        ErrorKind::Runtime
    );
}

// ── Hash slice ──

#[test]
fn hash_slice_string_range_quoted() {
    assert_eq!(
        eval_string(r#"my %h=(a=>1,b=>2,c=>3,d=>4); join(",", @h{"a":"c"})"#),
        "1,2,3"
    );
}

#[test]
fn hash_slice_string_range_bareword_autoquote() {
    assert_eq!(
        eval_string(r#"my %h=(a=>1,b=>2,c=>3,d=>4); join(",", @h{a:c})"#),
        "1,2,3"
    );
}

#[test]
fn hash_slice_string_range_with_step() {
    assert_eq!(
        eval_string(r#"my %h=(a=>1,b=>2,c=>3,d=>4); join(",", @h{a:c:1})"#),
        "1,2,3"
    );
}

#[test]
fn hash_slice_numeric_range_stringifies() {
    assert_eq!(
        eval_string(r#"my %h=("1"=>10,"2"=>20,"3"=>30); join(",", @h{1:3})"#),
        "10,20,30"
    );
}

#[test]
fn hash_slice_open_ended_aborts() {
    assert_eq!(
        eval_err_kind(r#"my %h=(a=>1,b=>2); my @s = @h{a:}; 0"#),
        ErrorKind::Runtime
    );
}

#[test]
fn hash_slice_full_open_aborts() {
    assert_eq!(
        eval_err_kind(r#"my %h=(a=>1); my @s = @h{::}; 0"#),
        ErrorKind::Runtime
    );
}

// ── Mixed sigil contexts: `..` still produces a Range (legacy path); `:` now goes
//    through ArraySliceRange/HashSliceRange. Both must give the same result for
//    closed integer ranges. ──

#[test]
fn array_slice_double_dot_matches_colon() {
    let dots = eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[1..3])"#);
    let cols = eval_string(r#"my @a=(10,20,30,40,50); join(",", @a[1:3])"#);
    assert_eq!(dots, cols);
    assert_eq!(dots, "20,30,40");
}

// ── Edge cases: empty arrays, single-element arrays, length-clamped behavior ──

#[test]
fn array_slice_empty_array_full_returns_empty() {
    assert_eq!(eval_int(r#"my @a=(); my @r = @a[::]; len(@r)"#), 0);
}

#[test]
fn array_slice_empty_array_reversed_returns_empty() {
    assert_eq!(eval_int(r#"my @a=(); my @r = @a[::-1]; len(@r)"#), 0);
}

#[test]
fn array_slice_single_element_reversed() {
    assert_eq!(eval_int(r#"my @a=(42); my @r = @a[::-1]; $r[0]"#), 42);
}

#[test]
fn array_slice_single_element_full() {
    assert_eq!(eval_int(r#"my @a=(42); my @r = @a[::]; $r[0]"#), 42);
}

#[test]
fn array_slice_step_larger_than_length_returns_first() {
    // `@a[::100]` with default from=0, to=len-1, step=100 — only index 0 fits.
    assert_eq!(
        eval_string(r#"my @a=(10,20,30); my @r = @a[::100]; join(",", @r)"#),
        "10"
    );
}

#[test]
fn hash_slice_single_key_range_returns_one_value() {
    // `{a:a}` is a one-element string range. Auto-quoted barewords resolve
    // to the same key, lookup returns the value.
    assert_eq!(
        eval_int(r#"my %h=(a=>42, b=>99); my @r = @h{a:a}; $r[0]"#),
        42
    );
}

#[test]
fn array_slice_negative_index_clamps_to_zero() {
    // `@a[-2:-1]` on a 5-element array: -2 normalizes to 3, -1 normalizes to 4.
    // Both inclusive, step=1 → indices 3,4.
    assert_eq!(
        eval_string(r#"my @a=(10,20,30,40,50); my @r = @a[-2:-1]; join(",", @r)"#),
        "40,50"
    );
}

#[test]
fn array_slice_full_reverse_of_two_element_array() {
    assert_eq!(
        eval_string(r#"my @a=(10,20); my @r = @a[::-1]; join(",", @r)"#),
        "20,10"
    );
}

#[test]
fn hash_slice_numeric_range_with_negative_step_aborts_on_open_end() {
    // `@h{:5}` is open-start which is invalid for hashes (not the same as array).
    assert_eq!(
        eval_err_kind(r#"my %h=("1"=>1, "2"=>2); my @s = @h{:5}; 0"#),
        ErrorKind::Runtime
    );
}
