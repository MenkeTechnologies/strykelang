//! Pins for `len(EXPR)` semantics — the stryke-side codepoint/element
//! counter. The 2026-05-15 fakery audit found three test files in
//! `examples/` whose authors confused `len` (outer container count or
//! codepoint string length) with a flattened element count. These
//! pins lock in the actual semantics so the same Gemini-style
//! confusion can't slip past CI again.

use crate::common::*;

// ── Arrays ────────────────────────────────────────────────────────────

#[test]
fn len_of_array_returns_element_count() {
    assert_eq!(eval_int("my @a = (1, 2, 3, 4, 5); len(@a)"), 5);
}

#[test]
fn len_of_empty_array_is_zero() {
    assert_eq!(eval_int("my @a; len(@a)"), 0);
}

#[test]
fn len_of_arrayref_literal_counts_inner() {
    // `[1, 2, 3]` is an arrayref; `len` reads through to inner count.
    assert_eq!(eval_int("len([1, 2, 3])"), 3);
}

#[test]
fn len_of_arrayref_in_scalar_counts_inner() {
    // `len($aref)` ≡ `len(@$aref)` — both yield the inner element count.
    assert_eq!(eval_int("my $a = [10, 20, 30]; len($a)"), 3);
}

#[test]
fn len_of_explicit_arrayref_deref_counts_inner() {
    assert_eq!(eval_int("my $a = [10, 20, 30]; len(@$a)"), 3);
}

// ── Nested arrays — outer count, NOT flattened ────────────────────────
// This is the fakery point from the audit. Authors writing
// `len([[1,2],[3,4]]) == 4` confuse outer-count with flat count.

#[test]
fn len_of_nested_arrayref_returns_outer_count_not_flat() {
    // Outer arrayref has 2 inner arrayrefs → len = 2 (NOT 4).
    assert_eq!(eval_int("len([[1, 2], [3, 4]])"), 2);
}

#[test]
fn len_of_three_row_matrix_returns_three() {
    // 3×2 matrix → 3 rows. Flat count would be 6.
    assert_eq!(eval_int("len([[1, 2], [3, 4], [5, 6]])"), 3);
}

#[test]
fn len_of_named_array_of_arrayrefs_returns_outer_count() {
    assert_eq!(
        eval_int("my @rows = ([1, 2], [3, 4], [5, 6, 7]); len(@rows)"),
        3
    );
}

// ── Hashes ────────────────────────────────────────────────────────────

#[test]
fn len_of_hash_returns_key_count() {
    // The audit's `len(%h) == 4` (k+v flat) fakery is invalid.
    assert_eq!(eval_int("my %h = (a => 1, b => 2, c => 3); len(%h)"), 3);
}

#[test]
fn len_of_empty_hash_is_zero() {
    assert_eq!(eval_int("my %h; len(%h)"), 0);
}

#[test]
fn len_of_explicit_hashref_deref_returns_key_count() {
    assert_eq!(eval_int("my $h = +{ a => 1, b => 2 }; len(%$h)"), 2);
}

// ── Strings — codepoints, NOT bytes ───────────────────────────────────
// Pinned consistently with `docs/index.html` String Coordinates table:
// stryke `len` is codepoint-indexed; Perl `length` is byte-indexed.

#[test]
fn len_of_ascii_string_returns_char_count() {
    assert_eq!(eval_int("len(\"hello\")"), 5);
}

#[test]
fn len_of_unicode_string_returns_codepoints_not_bytes() {
    // `café` is 4 codepoints, 5 bytes in UTF-8. `len` is codepoint-aware.
    assert_eq!(eval_int("len(\"café\")"), 4);
}

#[test]
fn len_of_emoji_string_counts_each_emoji_as_one() {
    // "🔑" is 1 codepoint (4 bytes in UTF-8). Compare with `length`
    // which returns the byte count.
    let code = r#"
        my $s = "🔑";
        (len($s) == 1 && length($s) == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn len_of_empty_string_is_zero() {
    assert_eq!(eval_int("len(\"\")"), 0);
}

// ── Stryke colon-range `N:M` ──────────────────────────────────────────

#[test]
fn len_of_inclusive_colon_range_counts_endpoints() {
    // `1:5` is inclusive on both sides → 5 elements.
    assert_eq!(eval_int("len(1:5)"), 5);
}

#[test]
fn len_of_zero_based_colon_range() {
    assert_eq!(eval_int("len(0:9)"), 10);
}

#[test]
fn len_of_single_point_colon_range_is_one() {
    assert_eq!(eval_int("len(0:0)"), 1);
}

// ── Pipe-forward ──────────────────────────────────────────────────────

#[test]
fn pipe_forward_into_len_works_on_arrayref() {
    assert_eq!(eval_int("[1, 2, 3, 4] |> len"), 4);
}

#[test]
fn pipe_forward_into_len_works_on_string() {
    assert_eq!(eval_int("\"abcdef\" |> len"), 6);
}

#[test]
fn pipe_forward_into_len_chains_with_grep() {
    assert_eq!(
        eval_int("(1:10) |> grep { _ % 2 == 0 } |> len"),
        5
    );
}

// ── Hands-off the dispatch path ───────────────────────────────────────
// `len` is the canonical builtin; ensure the standard call form
// matches the pipe-forward form for the same value.

#[test]
fn len_call_form_matches_pipe_forward_form() {
    let code = r#"
        my @a = (10, 20, 30);
        my $direct = len(@a);
        my $piped  = @a |> len;
        $direct == $piped ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pinning the audit findings concretely ────────────────────────────

#[test]
fn pauli_x_matrix_len_is_two_not_flat_four() {
    // `pauli_x()` returns `[[0,1],[1,0]]` — len = 2 outer rows.
    assert_eq!(eval_int("len(pauli_x())"), 2);
}

#[test]
fn box_blur_kernel_3_len_is_seven_not_flat_49() {
    // `(2r+1)=7` rows. Flattened weight count is 49 but you have to
    // sum row lengths to get there; `len` itself gives 7.
    assert_eq!(eval_int("len(box_blur_kernel(3))"), 7);
}

#[test]
fn lu_decompose_returns_three_pieces() {
    // `[L, U, P]` — len = 3. Not a flattened 6 or 12.
    assert_eq!(eval_int("len(lu_decompose([[1,2],[3,4]]))"), 3);
}
