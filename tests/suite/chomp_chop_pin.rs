//! `chomp` and `chop` semantics pins.

use crate::common::*;

// ── chomp on scalar ────────────────────────────────────────────────

#[test]
fn chomp_removes_trailing_newline() {
    let code = r#"
        my $s = "hello\n";
        chomp($s);
        ($s eq "hello" && length($s) == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_no_newline_is_noop() {
    let code = r#"
        my $s = "world";
        my $r = chomp($s);
        ($r == 0 && $s eq "world") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_returns_count_of_chars_removed() {
    let code = r#"
        my $s = "x\n";
        my $r = chomp($s);
        $r == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_only_removes_one_newline() {
    let code = r#"
        my $s = "hi\n\n";
        my $r = chomp($s);
        # Only the trailing \n is removed; "hi\n" remains.
        ($r == 1 && length($s) == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_preserves_carriage_return() {
    let code = r#"
        # Default $/ is "\n", so \r is left.
        my $s = "hi\r\n";
        chomp($s);
        length($s) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_empty_string_is_noop() {
    let code = r#"
        my $s = "";
        my $r = chomp($s);
        ($r == 0 && $s eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_string_thats_only_newline_yields_empty() {
    let code = r#"
        my $s = "\n";
        chomp($s);
        ($s eq "" && length($s) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chomp on array ─────────────────────────────────────────────────

#[test]
fn chomp_array_strips_newlines_from_each() {
    let code = r#"
        my @arr = ("a\n", "b\n", "c\n");
        chomp(@arr);
        join("|", @arr) eq "a|b|c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_array_returns_total_chars_removed() {
    let code = r#"
        my @arr = ("a\n", "b\n", "c\n");
        my $r = chomp(@arr);
        $r == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_array_mixed_newlines() {
    let code = r#"
        my @arr = ("a", "b\n", "c", "d\n");
        my $r = chomp(@arr);
        # 2 newlines removed; results: a, b, c, d.
        ($r == 2 && join(",", @arr) eq "a,b,c,d") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_array_empty() {
    let code = r#"
        my @arr;
        my $r = chomp(@arr);
        $r == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chomp via for-alias ────────────────────────────────────────────

#[test]
fn chomp_inside_for_alias_modifies_array() {
    let code = r#"
        my @arr = ("foo\n", "bar\n", "baz\n");
        for my $line (@arr) {
            chomp($line);
        }
        join(",", @arr) eq "foo,bar,baz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chop on scalar ─────────────────────────────────────────────────

#[test]
fn chop_removes_last_char() {
    let code = r#"
        my $s = "hello";
        chop($s);
        $s eq "hell" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_returns_removed_char() {
    let code = r#"
        my $s = "hello";
        my $r = chop($s);
        $r eq "o" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_empty_returns_empty() {
    let code = r#"
        my $s = "";
        my $r = chop($s);
        ($r eq "" && $s eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_single_char_yields_empty() {
    let code = r#"
        my $s = "x";
        my $r = chop($s);
        ($r eq "x" && $s eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_removes_newline_too() {
    let code = r#"
        my $s = "hi\n";
        my $r = chop($s);
        ($r eq "\n" && $s eq "hi") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chop on array ──────────────────────────────────────────────────

#[test]
fn chop_array_strips_last_char_per_element() {
    let code = r#"
        my @arr = ("abc", "def", "ghi");
        chop(@arr);
        join(",", @arr) eq "ab,de,gh" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_array_returns_last_char_overall() {
    let code = r#"
        my @arr = ("ab", "cd", "ef");
        my $r = chop(@arr);
        # In Perl, chop(@arr) returns the last character removed
        # across the array, which is "f".
        $r eq "f" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── line-stripping idiom ─────────────────────────────────────────

#[test]
fn split_lines_and_chomp_each() {
    let code = r#"
        my $blob = "line1\nline2\nline3\n";
        my @lines = split /\n/, $blob;
        chomp(@lines);
        # split removes the \n already; chomp is a no-op.
        join(",", @lines) eq "line1,line2,line3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chomp_after_explicit_read_style() {
    let code = r#"
        # Simulate reading lines that come with trailing newlines.
        my @raw = map { "$_\n" } ("a", "b", "c");
        chomp(@raw);
        join(",", @raw) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chop vs chomp on terminator ──────────────────────────────────

#[test]
fn chop_vs_chomp_on_newline() {
    let code = r#"
        my $a = "abc\n";
        my $b = "abc\n";
        chomp($a);     # removes \n only
        chop($b);      # removes \n
        ($a eq "abc" && $b eq "abc") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chop_vs_chomp_on_no_newline() {
    let code = r#"
        my $a = "abc";
        my $b = "abc";
        chomp($a);     # noop
        chop($b);      # removes 'c'
        ($a eq "abc" && $b eq "ab") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chained chops ──────────────────────────────────────────────────

#[test]
fn chained_chops_collapse_string() {
    let code = r#"
        my $s = "abcdef";
        chop($s); chop($s); chop($s);
        $s eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn chained_chomps_only_strip_once() {
    let code = r#"
        my $s = "abc\n";
        chomp($s);
        chomp($s);   # already stripped; noop
        chomp($s);
        $s eq "abc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chop on UTF-8 multi-byte ──────────────────────────────────────

#[test]
fn chop_on_snowman_string() {
    let code = r#"
        my $s = "hi\x{2603}";
        my $r = chop($s);
        # The trailing char is the snowman (☃); $s left with "hi".
        ($s eq "hi" && $r eq "\x{2603}") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chomp with explicit $/ ────────────────────────────────────────

#[test]
fn chomp_does_not_honor_local_record_separator_per_bug_250() {
    // Stryke surface: `chomp` always strips a trailing `\n` regardless
    // of `local $/`. In Perl, `local $/ = "END"` would make chomp strip
    // the "END" suffix. See BUG-250.
    let code = r#"
        local $/ = "END";
        my $s = "dataEND";
        chomp($s);
        # Untouched per BUG-250.
        ($s eq "dataEND" && length($s) == 7) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── 10k line stripping ────────────────────────────────────────────

#[test]
fn chomp_10k_lines_strips_all() {
    let code = r#"
        my @lines = map { "$_\n" } (1:10000);
        my $r = chomp(@lines);
        ($r == 10000 && len(@lines) == 10000 && $lines[9999] eq "10000") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chomp + chop interaction ──────────────────────────────────────

#[test]
fn chomp_then_chop() {
    let code = r#"
        my $s = "abc\n";
        chomp($s);
        chop($s);
        $s eq "ab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
