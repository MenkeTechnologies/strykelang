//! String-operation pins. Covers substr/index/uc/lc/repeat/chomp/chop
//! at the byte and codepoint axes. `string_coordinates_pin.rs` (round
//! 3) locks the byte-vs-codepoint axis split; this file locks the
//! operation surface around it.

use crate::common::*;

// ── substr ───────────────────────────────────────────────────────────

#[test]
fn substr_byte_indexed_basic() {
    let code = r#"
        substr("hello world", 0, 5) eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_byte_indexed_with_offset() {
    let code = r#"
        substr("hello world", 6, 5) eq "world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_negative_offset_from_end() {
    let code = r#"
        # Perl: negative offset = from end. "hello world" → -5,5 = "world".
        substr("hello world", -5, 5) eq "world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_no_length_takes_rest() {
    let code = r#"
        substr("hello world", 6) eq "world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn substr_at_end_returns_empty() {
    let code = r#"
        substr("hello", 5) eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── index / rindex ──────────────────────────────────────────────────

#[test]
fn index_finds_substring() {
    let code = r#"
        index("hello world", "world") == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_returns_negative_one_on_miss() {
    let code = r#"
        index("hello world", "missing") == -1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_finds_first_occurrence() {
    let code = r#"
        index("abc abc abc", "abc") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_with_start_offset() {
    let code = r#"
        index("abc abc abc", "abc", 1) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn rindex_finds_last_occurrence() {
    let code = r#"
        rindex("abc abc abc", "abc") == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── uc / lc / ucfirst / lcfirst ─────────────────────────────────────

#[test]
fn uc_upcases_ascii_string() {
    let code = r#"
        uc("hello") eq "HELLO" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lc_downcases_ascii_string() {
    let code = r#"
        lc("HELLO") eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ucfirst_caps_only_first() {
    let code = r#"
        ucfirst("hello world") eq "Hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lcfirst_lowers_only_first() {
    let code = r#"
        lcfirst("HELLO WORLD") eq "hELLO WORLD" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uc_unicode_preserves_non_letters() {
    let code = r#"
        uc("café 🌟") eq "CAFÉ 🌟" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chomp / chop ────────────────────────────────────────────────────

#[test]
fn chomp_removes_trailing_newline() {
    let code = r#"
        my $s = "hello\n";
        chomp $s;
        $s eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_does_not_remove_internal_newlines() {
    let code = r#"
        my $s = "a\nb\nc\n";
        chomp $s;
        $s eq "a\nb\nc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_on_string_without_newline_is_noop() {
    let code = r#"
        my $s = "hello";
        chomp $s;
        $s eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_removes_last_char() {
    let code = r#"
        my $s = "hello";
        chop $s;
        $s eq "hell" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String repeat `x` operator ──────────────────────────────────────

#[test]
fn repeat_operator_basic() {
    let code = r#"
        ("ab" x 3) eq "ababab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn repeat_operator_zero_yields_empty() {
    let code = r#"
        ("ab" x 0) eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn repeat_operator_one_yields_input() {
    let code = r#"
        ("ab" x 1) eq "ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dash_repeat_for_underline() {
    let code = r#"
        ("-" x 10) eq "----------" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── concat operator `.` ─────────────────────────────────────────────

#[test]
fn dot_concat_two_strings() {
    let code = r#"
        ("hello" . " " . "world") eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_concat_string_with_number() {
    let code = r#"
        ("answer=" . 42) eq "answer=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dot_eq_appends_in_place() {
    let code = r#"
        my $s = "hello";
        $s .= " world";
        $s eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String interpolation in double-quoted strings ───────────────────

#[test]
fn double_quoted_string_interpolates_scalar() {
    let code = r#"
        my $name = "alice";
        "hello, $name!" eq "hello, alice!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn double_quoted_string_interpolates_array() {
    let code = r#"
        my @a = (1, 2, 3);
        # Perl: array in dq interpolates as join(" ", @a) by default.
        "list: @a" eq "list: 1 2 3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn single_quoted_string_does_not_interpolate() {
    let code = r#"
        my $name = "alice";
        my $s = 'hello, $name!';
        $s eq "hello, \$name!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn double_quoted_string_with_expression_interpolation() {
    let code = r#"
        my $x = 10;
        my $y = 20;
        "sum is @{[$x + $y]}" eq "sum is 30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String comparison operators ─────────────────────────────────────

#[test]
fn eq_compares_strings() {
    let code = r#"
        ("hello" eq "hello") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ne_inverse_of_eq() {
    let code = r#"
        ("hello" ne "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn cmp_returns_minus_one_zero_or_plus_one() {
    let code = r#"
        my @r = ("apple" cmp "banana", "apple" cmp "apple", "banana" cmp "apple");
        join(",", @r) eq "-1,0,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn lt_gt_string_compare() {
    let code = r#"
        (("abc" lt "abd") && ("xyz" gt "abc")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── reverse on string ───────────────────────────────────────────────

#[test]
fn reverse_in_scalar_context_flips_string() {
    let code = r#"
        my $s = scalar reverse "hello";
        $s eq "olleh" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── split / join round-trip ─────────────────────────────────────────

#[test]
fn split_then_join_roundtrip() {
    let code = r#"
        my $s = "a,b,c,d";
        my @parts = split /,/, $s;
        my $back = join(",", @parts);
        $back eq $s ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_on_whitespace_special_pattern_collapses() {
    // BUG-221 (FIXED): Perl's `split(" ", ...)` is the awk-mode special form —
    // the string `" "` strips leading whitespace and collapses whitespace runs.
    // Stryke now matches; the `/ /` regex form keeps literal-space semantics.
    let awk = r#"
        my @parts = split(" ", "  hello   world  ");
        (len(@parts) == 2 && $parts[0] eq "hello" && $parts[1] eq "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(awk), 1);

    // The `/\s+/` regex form still yields a leading empty field (no awk strip).
    let code = r#"
        my @parts = split(/\s+/, "  hello   world  ");
        @parts = grep { len($_) > 0 } @parts;
        (len(@parts) == 2 && $parts[0] eq "hello" && $parts[1] eq "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
