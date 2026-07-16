//! Pin `split` behavior: separator handling, trailing-empty trimming,
//! limit semantics, char-by-char split, regex-with-capture handling.
//! Every assertion below was probed against the running interpreter on
//! 2026-05-23 before being pinned, so a regression here flags an
//! intentional or accidental behavioral change.

use crate::common::*;

#[test]
fn split_drops_trailing_empties_by_default() {
    // Trailing empty fields are dropped when no limit is supplied.
    let code = r#"
        my @r = split(/,/, "a,b,c,,");
        len(@r)
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn split_negative_limit_preserves_trailing_empties() {
    // Limit < 0 keeps every trailing empty field.
    let code = r#"
        my @r = split(/,/, "a,b,c,,", -1);
        join("|", @r) eq "a|b|c||" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_positive_limit_caps_field_count() {
    // Limit = 2 returns exactly 2 fields; the tail is left intact in
    // the last element.
    let code = r#"
        my @r = split(/,/, "a,b,c,d", 2);
        len(@r) == 2 && $r[0] eq "a" && $r[1] eq "b,c,d" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_empty_pattern_yields_chars() {
    // `split //` returns one element per character.
    let code = r#"
        my @r = split(//, "abc");
        len(@r) == 3 && $r[0] eq "a" && $r[2] eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_empty_input_yields_empty_list() {
    let code = r#"
        my @r = split(/,/, "");
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn split_no_match_returns_whole_string() {
    let code = r#"
        my @r = split(/;/, "abc");
        len(@r) == 1 && $r[0] eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_on_whitespace_regex_collapses_runs() {
    // Splitting on `\s+` with a leading whitespace tail produces an
    // initial empty field — this differs from Perl's awk-mode
    // `split " "` and is the documented stryke contract.
    let code = r#"
        my @r = split(/\s+/, "  one  two   three");
        len(@r) == 4 && $r[0] eq "" && $r[1] eq "one" && $r[3] eq "three" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_capture_group_is_emitted_perl_style() {
    // A capturing pattern emits each capture as a field of its own, as perl
    // does: `split /(,)/, "a,b,c"` is (a, ",", b, ",", c). This lifts the
    // earlier pin, which documented the pre-fix contract of dropping the
    // captures entirely — the change that pin anticipated.
    let code = r#"
        my @r = split(/(,)/, "a,b,c");
        len(@r) == 5 && $r[0] eq "a" && $r[1] eq "," && $r[2] eq "b" && $r[3] eq "," && $r[4] eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_capture_group_non_participating_yields_undef() {
    // A group that did not take part in the match yields undef, not "".
    // `split /(a)|(b)/, "1a2"` is (1, "a", undef, 2).
    let code = r#"
        my @r = split(/(a)|(b)/, "1a2");
        len(@r) == 4 && $r[1] eq "a" && !defined($r[2]) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_capture_group_limit_counts_fields_only() {
    // LIMIT caps fields; captures are extra and never count toward it.
    // `split /(,)/, "a,b,c", 2` is (a, ",", "b,c").
    let code = r#"
        my @r = split(/(,)/, "a,b,c", 2);
        len(@r) == 3 && $r[0] eq "a" && $r[1] eq "," && $r[2] eq "b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_limit_one_returns_single_field() {
    // Limit = 1 short-circuits — no splitting performed.
    let code = r#"
        my @r = split(/,/, "a,b,c", 1);
        len(@r) == 1 && $r[0] eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_on_multichar_literal_via_regex() {
    let code = r#"
        my @r = split(/::/, "Foo::Bar::Baz");
        len(@r) == 3 && $r[1] eq "Bar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_preserves_internal_empties() {
    // Internal empties survive even without an explicit limit.
    let code = r#"
        my @r = split(/,/, "a,,b,,c");
        len(@r) == 5 && $r[1] eq "" && $r[3] eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_single_space_string_is_awk_mode() {
    // Perl awk-mode: the *string* " " strips leading whitespace and
    // splits on runs of whitespace — no leading empty, runs collapsed.
    // Contrast `split(/ /, ...)` / `split(/\s+/, ...)` which do not.
    let code = r#"
        my @r = split(" ", "  a   b  c ");
        len(@r) == 3 && $r[0] eq "a" && $r[1] eq "b" && $r[2] eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_single_space_string_collapses_tabs_and_newlines() {
    // awk-mode splits on `\s+`, not just literal spaces.
    let code = r#"
        my @r = split(" ", "\ta\tb\n");
        len(@r) == 2 && $r[0] eq "a" && $r[1] eq "b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_single_space_string_positive_limit_keeps_remainder() {
    // Leading strip applies, then LIMIT caps fields; the tail keeps its
    // internal and trailing whitespace verbatim.
    let code = r#"
        my @r = split(" ", "  a   b  c ", 2);
        len(@r) == 2 && $r[0] eq "a" && $r[1] eq "b  c " ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_single_space_string_negative_limit_keeps_trailing_empty() {
    // Negative LIMIT preserves the trailing empty produced by the
    // final whitespace run, just like Perl.
    let code = r#"
        my @r = split(" ", "  a   b  c ", -1);
        join("|", @r) eq "a|b|c|" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn split_single_space_string_all_whitespace_yields_empty() {
    // After the leading strip an all-whitespace string is empty → empty list.
    let code = r#"
        my @r = split(" ", "    ");
        len(@r)
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn split_space_regex_is_not_awk_mode() {
    // `split(/ /, ...)` keeps literal single-space semantics: leading
    // whitespace yields empty fields, runs are NOT collapsed. Guards
    // against the awk-mode branch leaking into the regex path.
    let code = r#"
        my @r = split(/ /, "  a b");
        len(@r) == 4 && $r[0] eq "" && $r[1] eq "" && $r[2] eq "a" && $r[3] eq "b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
