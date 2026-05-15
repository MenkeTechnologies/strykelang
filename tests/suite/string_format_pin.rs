//! String-literal-form pins: heredoc, q{}, qq{}, qw(), here-string.

use crate::common::*;

// ── Single-quoted string: no interpolation ─────────────────────────

#[test]
fn single_quoted_no_interpolation() {
    let code = r#"
        my $x = 42;
        my $s = 'value=$x';
        $s eq "value=\$x" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn single_quoted_preserves_special_chars() {
    let code = r#"
        my $s = 'a\nb';
        # No \n expansion — literal backslash+n.
        len($s) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Double-quoted string: interpolation ────────────────────────────

#[test]
fn double_quoted_interpolates_scalar() {
    let code = r#"
        my $x = 42;
        my $s = "value=$x";
        $s eq "value=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn double_quoted_interpolates_array_with_default_sep() {
    let code = r#"
        my @a = (1, 2, 3);
        my $s = "list: @a";
        $s eq "list: 1 2 3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn double_quoted_escape_sequences() {
    let code = r#"
        my $s = "a\nb\tc";
        len($s) == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn double_quoted_with_expression_interpolation() {
    let code = r#"
        my $x = 5;
        my $y = 7;
        my $s = "sum is @{[$x + $y]}";
        $s eq "sum is 12" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── q{...} = single-quote alternative ──────────────────────────────

#[test]
fn q_curly_no_interpolation() {
    let code = r#"
        my $x = 42;
        my $s = q{value=$x};
        $s eq "value=\$x" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn q_curly_can_contain_quotes() {
    let code = r#"
        my $s = q{he said "hi"};
        $s eq "he said \"hi\"" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── qq{...} = double-quote alternative ─────────────────────────────

#[test]
fn qq_curly_interpolates() {
    let code = r#"
        my $x = 42;
        my $s = qq{value=$x};
        $s eq "value=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qq_paren_alternative_delimiter() {
    let code = r#"
        my $x = "hello";
        my $s = qq($x world);
        $s eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── qw(...) = word list ───────────────────────────────────────────

#[test]
fn qw_paren_word_list() {
    let code = r#"
        my @w = qw(alpha beta gamma);
        len(@w) == 3 && $w[1] eq "beta" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qw_curly_word_list() {
    let code = r#"
        my @w = qw{one two three four};
        len(@w) == 4 && $w[3] eq "four" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qw_with_multiple_whitespace() {
    let code = r#"
        my @w = qw(  a   b   c  );
        # Whitespace collapses; 3 elements.
        len(@w) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-line strings ─────────────────────────────────────────────

#[test]
fn multiline_double_quoted_string() {
    let code = r#"
        my $s = "line1
line2
line3";
        # 3 lines = 2 newlines + content.
        my @ls = split /\n/, $s;
        len(@ls) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── q{} block-content with multi-line ──────────────────────────────

#[test]
fn q_curly_multiline() {
    let code = r#"
        my $s = q{
            line a
            line b
        };
        index($s, "line a") >= 0 && index($s, "line b") >= 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String concat behaves consistently ────────────────────────────

#[test]
fn concat_then_compare() {
    let code = r#"
        my $a = "hello";
        my $b = "world";
        ($a . " " . $b) eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String repeat operator ─────────────────────────────────────────

#[test]
fn string_repeat_x() {
    let code = r#"
        my $s = "ab" x 4;
        $s eq "abababab" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_repeat_x() {
    let code = r#"
        my @a = (0) x 5;
        (len(@a) == 5 && $a[0] == 0 && $a[4] == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── \0, \x{NN}, \uNNNN escapes ────────────────────────────────────

#[test]
fn null_byte_escape() {
    let code = r#"
        my $s = "a\0b";
        len($s) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hex_escape() {
    let code = r#"
        my $s = "\x41";   # 'A'
        $s eq "A" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn unicode_escape() {
    let code = r#"
        my $s = "\x{1F31F}";   # 🌟
        len($s) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── chr/ord round-trip ────────────────────────────────────────────

#[test]
fn chr_ord_roundtrip() {
    let code = r#"
        my $c = chr(65);
        ord($c) == 65 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String length: byte vs codepoint ──────────────────────────────

#[test]
fn length_byte_count() {
    let code = r#"
        # "café" = 5 bytes (é is 2-byte UTF-8) but 4 codepoints.
        length("café") == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn len_codepoint_count() {
    let code = r#"
        len("café") == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Hash-interpolation idiom (with sigil disambiguation) ──────────

#[test]
fn hash_interpolation_via_array_braces() {
    let code = r#"
        my %h = (a => 1, b => 2);
        # Interpolating %h directly isn't allowed in Perl; use @{[...]}.
        my $s = "{a=" . $h{a} . " b=" . $h{b} . "}";
        $s eq "{a=1 b=2}" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sprintf vs string concat equivalence ──────────────────────────

#[test]
fn sprintf_vs_concat_equivalent() {
    let code = r#"
        my $name = "alice";
        my $age = 30;
        my $a = "name=$name age=$age";
        my $b = sprintf("name=%s age=%d", $name, $age);
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── tr/y (transliteration) ────────────────────────────────────────

#[test]
fn tr_substitution_works() {
    let code = r#"
        my $s = "hello";
        $s =~ tr/a-z/A-Z/;
        $s eq "HELLO" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tr_returns_count_replaced() {
    let code = r#"
        my $s = "hello world";
        my $n = ($s =~ tr/l/L/);
        $n == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
