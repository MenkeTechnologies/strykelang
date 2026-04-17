//! Perl 5–style range and flip-flop operators in perlrs (`..`, `...`).
//!
//! ## List context (`..` / `...`)
//! - **Numeric expansion** when [`perlrs::value::perl_list_range_pair_is_numeric`] says so: inclusive
//!   integer sequence; **descending** numeric `(5..2)` is empty.
//! - **Magical string increment** otherwise (`perl_magic_string_increment_for_range` in `value.rs`):
//!   ASCII letter/digit tails advance like Perl `++`; iteration stops when the current string’s width
//!   exceeds the right endpoint’s (so `"c".."a"` walks `c..z`, not empty).
//! - Leading-zero strings such as `"01".."03"` stay in **string** mode (see `run_semantics_tests`).
//! - In list context, `...` parses as the three-dot token but **expands the same as** `..` for
//!   simple numeric literals (regression guard).
//!
//! ## Scalar context — numeric line flip-flop
//! - Driven by `$.` ([`perlrs::interpreter::Interpreter::scalar_flipflop_dot_line`]). Inactive → empty
//!   string (`""`, false). Active lines emit `"1"`, `"2"`, …; the **closing** line uses a `"E0"` suffix
//!   (Perl `pp_flop` stringification).
//!
//! ## Scalar context — regex flip-flop
//! - `/a/../b/`, `/a/.../b/`, compound RHS, and `m{...}...eof` are covered in [`super::regex`] and
//!   [`super::cli_line_mode_eof`].

use crate::common::{eval_int, eval_string};

#[test]
fn list_range_three_dot_numeric_same_expansion_as_two_dot() {
    assert_eq!(
        eval_string(r#"join ",", (1...5)"#),
        eval_string(r#"join ",", (1..5)"#)
    );
    assert_eq!(eval_string(r#"join ",", (1...5)"#), "1,2,3,4,5");
}

#[test]
fn list_range_uppercase_letters() {
    assert_eq!(eval_string(r#"join ",", ("A".."C")"#), "A,B,C");
}

#[test]
fn list_range_magic_increment_z_to_aa_inclusive() {
    assert_eq!(eval_string(r#"join ",", ("z".."aa")"#), "z,aa");
}

#[test]
fn list_range_undef_left_uses_zero_numeric() {
    assert_eq!(eval_string(r#"join ",", (undef..3)"#), "0,1,2,3");
}

#[test]
fn list_range_undef_right_yields_empty_numeric() {
    assert_eq!(eval_int(r#"my @x = (3..undef); 0+@x"#), 0);
}

#[test]
fn list_range_mixed_int_and_numeric_string_endpoints() {
    assert_eq!(eval_string(r#"join ",", (1 .. "03")"#), "1,2,3");
    assert_eq!(eval_string(r#"join ",", ("02" .. 4)"#), "2,3,4");
}

/// Descending **numeric** range is empty; descending **string** endpoints still magic-walk forward.
#[test]
fn list_range_string_c_to_a_is_magic_forward_not_empty() {
    assert_eq!(eval_int(r#"my @x = ("c".."a"); 0+@x"#), 24);
    assert_eq!(
        eval_string(r#"my @x = ("c".."a"); join "-", $x[0], $x[-1]"#),
        "c-z"
    );
}

/// Sed-style numeric flip-flop on `$.`: inactive lines stringify empty; close line uses `E0`.
#[test]
fn scalar_numeric_flip_flop_sequence_stringifies_like_perl() {
    assert_eq!(
        eval_string(
            r#"my $acc = "";
            for my $i (1..6) {
              $. = $i;
              $acc .= "[" . (3..5) . "]";
            }
            $acc"#,
        ),
        "[][][1][2][3E0][]"
    );
}

#[test]
fn scalar_numeric_flip_flop_truthy_only_while_active() {
    assert_eq!(
        eval_string(
            r#"my $acc = "";
            for my $i (1..6) {
              $. = $i;
              $acc .= (3..5) ne "" ? "T" : "F";
            }
            $acc"#,
        ),
        "FFTTTF"
    );
}
