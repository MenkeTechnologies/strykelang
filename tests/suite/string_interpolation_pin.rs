//! String interpolation pins — qq//, "$x", "${x}", "@arr", "$arr[0]",
//! "$h{k}", "@{[ expr ]}", "${\ expr }", slice, negative-index, braced
//! disambiguation. Complements string_concat_pin / string_format_pin.

use crate::common::*;

// ── scalar interpolation ───────────────────────────────────────────

#[test]
fn scalar_var_in_double_quoted() {
    let code = r#"
        my $x = 42;
        my $s = "x=$x";
        $s eq "x=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn scalar_braced_disambiguation() {
    let code = r#"
        my $name = "Jacob";
        my $s = "${name}er";   # "Jacober"
        $s eq "Jacober" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn scalar_followed_by_punctuation() {
    let code = r#"
        my $name = "Jacob";
        my $s = "$name!";       # ! is not an ident char, no braces needed
        $s eq "Jacob!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── q vs qq ────────────────────────────────────────────────────────

#[test]
fn single_quote_does_not_interpolate() {
    let code = r#"
        my $x = 5;
        my $s = '$x is literal';
        $s eq '$x is literal' ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn q_paren_form_no_interpolation() {
    let code = r#"
        my $x = 5;
        my $s = q($x is literal);
        $s eq '$x is literal' ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qq_paren_form_does_interpolate() {
    let code = r#"
        my $x = 5;
        my $s = qq($x is interp);
        $s eq '5 is interp' ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── array interpolation ────────────────────────────────────────────

#[test]
fn array_interpolates_with_space_separator() {
    let code = r#"
        my @a = (1, 2, 3);
        my $s = "list=@a end";
        $s eq "list=1 2 3 end" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_element_via_dollar_bracket() {
    let code = r#"
        my @a = (10, 20, 30);
        my $s = "first=$a[0] second=$a[1]";
        $s eq "first=10 second=20" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_negative_index_interpolates() {
    let code = r#"
        my @a = (10, 20, 30);
        my $s = "last=$a[-1]";
        $s eq "last=30" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_interpolates() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        my $s = "mid=@a[1..3]";
        $s eq "mid=2 3 4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn empty_array_interpolates_to_empty() {
    let code = r#"
        my @empty;
        my $s = "x=@empty y";
        $s eq "x= y" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── hash interpolation ────────────────────────────────────────────

#[test]
fn hash_value_via_dollar_brace() {
    let code = r#"
        my %h = (k => 99);
        my $s = "v=$h{k}";
        $s eq "v=99" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_value_with_string_key_in_qq() {
    let code = r#"
        my %h = (alpha => 1, beta => 2);
        my $s = "a=$h{alpha} b=$h{beta}";
        $s eq "a=1 b=2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── expression interpolation ────────────────────────────────────────

#[test]
fn array_block_interpolates_expression_list() {
    let code = r#"
        my @a = (1, 2, 3);
        my $s = "x=@{[ map { _ * 2 } @a ]} end";
        $s eq "x=2 4 6 end" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn scalar_ref_block_interpolates_single_value() {
    let code = r#"
        my $s = "sum=${\ (1 + 2 + 3) }";
        $s eq "sum=6" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn scalar_ref_block_with_fn_call() {
    let code = r#"
        fn Demo::SI::pi() { 3.14 }
        my $s = "pi=${\ Demo::SI::pi() }";
        $s eq "pi=3.14" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── escape sequences ───────────────────────────────────────────────

#[test]
fn newline_escape_in_double_quoted() {
    let code = r#"
        my $s = "a\nb";
        length($s) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tab_escape_in_double_quoted() {
    let code = r#"
        my $s = "a\tb";
        length($s) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn escaped_dollar_is_literal() {
    let code = r#"
        my $x = 5;
        my $s = "\$x=$x";
        $s eq '$x=5' ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn escaped_at_is_literal() {
    let code = r#"
        my @a = (1, 2);
        my $s = "\@a=@a";
        $s eq '@a=1 2' ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── concatenation interplay ────────────────────────────────────────

#[test]
fn interpolation_inside_concat() {
    let code = r#"
        my $x = "foo";
        my $y = "bar";
        my $s = "$x" . "_" . "$y";
        $s eq "foo_bar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn heredoc_interpolates() {
    let code = r#"
        my $name = "stryke";
        my $body = <<END;
hello $name
END
        $body eq "hello stryke\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn heredoc_single_quoted_tag_no_interp() {
    let code = r#"
        my $name = "stryke";
        my $body = <<'END';
hello $name
END
        $body eq "hello \$name\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── refs interpolate to their string form ─────────────────────────

#[test]
fn array_ref_string_form_in_interp() {
    let code = r#"
        my @arr = (1, 2, 3);
        my $r = \@arr;
        # "$r" stringifies to "ARRAY(0x...)"; verify prefix only.
        my $s = "$r";
        $s =~ /^ARRAY\(0x/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_ref_string_form_in_interp() {
    let code = r#"
        my %h = (k => 1);
        my $r = \%h;
        my $s = "$r";
        $s =~ /^HASH\(0x/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn coderef_string_form_is_code_anon_not_hex_addr() {
    // Stryke surface: coderefs stringify as `CODE(__ANON__)` instead
    // of Perl's `CODE(0x<addr>)`. Documented as BUG-245.
    let code = r#"
        my $c = sub { 1 };
        my $s = "$c";
        $s eq "CODE(__ANON__)" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── undef interpolates to empty string ───────────────────────────

#[test]
fn undef_interpolates_to_empty_in_qq() {
    let code = r#"
        my $u;
        my $s = "[$u]";
        $s eq "[]" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── nested deref via $$ ────────────────────────────────────────────

#[test]
fn scalar_ref_double_dollar_does_not_deref_in_interp() {
    // Stryke surface: `$$r` inside double-quoted string interpolates
    // `$r` (the ref's stringification) followed by literal `$r`-like
    // body — yielding `SCALAR(0x...)` instead of the dereferenced
    // value. Workaround: `"${\ $$r }"`. Documented as BUG-246.
    let code = r#"
        my $x = 7;
        my $r = \$x;
        my $s = "val=$$r";
        # Observed: contains "SCALAR("; never "val=7".
        ($s =~ /SCALAR\(/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn scalar_ref_deref_works_via_backslash_block() {
    // Working idiom for deref in qq: `${\ EXPR }`.
    let code = r#"
        my $x = 7;
        my $r = \$x;
        my $s = "val=${\ $$r }";
        $s eq "val=7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── array of strings joined ────────────────────────────────────────

#[test]
fn array_join_separator_via_local_sep_alt() {
    let code = r#"
        my @words = ("alpha", "beta", "gamma");
        # Easiest portable form is join + interp.
        my $s = "result: " . join(",", @words);
        $s eq "result: alpha,beta,gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── sprintf parity ────────────────────────────────────────────────

#[test]
fn sprintf_vs_interpolation_match() {
    let code = r#"
        my $x = 42;
        my $a = "x=$x";
        my $b = sprintf("x=%d", $x);
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn interp_with_arithmetic_via_block() {
    let code = r#"
        my $n = 4;
        my $s = "sq=${\ ($n * $n) }";
        $s eq "sq=16" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── unicode literal in interp ─────────────────────────────────────

#[test]
fn unicode_interp_length_is_byte_count() {
    // Stryke `length` returns byte-count, not char-count: 8 prefix
    // bytes + 3 UTF-8 bytes for ☃ = 11. Perl with `use utf8` would
    // return 9 (char-count). Pinned per BUG-247.
    let code = r#"
        my $name = "\x{2603}";   # ☃
        my $s = "snowman:$name";
        length($s) == 11 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
