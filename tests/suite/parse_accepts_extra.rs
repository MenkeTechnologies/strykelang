//! Additional parse-accept cases (one `#[test]` per snippet; no macro batching).

fn p(src: &str) {
    stryke::parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"));
}

#[test]
fn accepts_m_brace_delimiter() {
    p("m{pat}");
}

#[test]
fn accepts_s_angle_delimiters() {
    p("s<old><new>");
}

#[test]
fn accepts_tr_slashes() {
    p("tr/a-z/A-Z/");
}

#[test]
fn accepts_q_slash() {
    p(r"q/slash/");
}

#[test]
fn accepts_qq_brackets() {
    p("qq[bracket]");
}

#[test]
fn accepts_qw_whitespace_separated() {
    p("my @z = qw( one two three )");
}

#[test]
fn accepts_heredoc_style_not_used_but_double_quote() {
    p(r#""end of string""#);
}

#[test]
fn accepts_sub_named_with_block() {
    p("sub name { 1; }");
}

#[test]
fn accepts_coderef_scalar_assignment() {
    p("my $c = fn { 1 }");
}

#[test]
fn accepts_state_operator_keyword() {
    p("state $st");
}

#[test]
fn accepts_constant_via_use_constant() {
    p("use constant PI => 4 * atan2(1, 1)");
}

#[test]
fn accepts_printf_line() {
    p(r#"printf "%s\n", "out""#);
}

#[test]
fn accepts_pod_like_line_starting_equals() {
    p("=cut\n1");
}

#[test]
fn accepts_x_repeat_operator_precedence() {
    p(r#""ab" x 2 + 1"#);
}

#[test]
fn accepts_list_context_paren() {
    p("my @x = (1, (2, 3))");
}

#[test]
fn accepts_hash_slice_syntax() {
    p("@h{'a', 'b'}");
}

#[test]
fn accepts_array_slice_syntax() {
    p("@a[0, 1, 2]");
}

#[test]
fn accepts_prototype_parens_on_sub() {
    p("sub sum ($$) { $_0 + $_1; }");
}

#[test]
fn accepts_caller_with_args() {
    p("caller(0)");
}

#[test]
fn accepts_wantarray_in_expression() {
    p("my $w = wantarray");
}

#[test]
fn accepts_eval_block_with_local() {
    p("eval { local $_ = 1; 1; }");
}

#[test]
fn accepts_require_version() {
    p("require v5.10");
}

#[test]
fn accepts_no_feature() {
    p("no feature 'say'");
}

#[test]
fn accepts_package_block() {
    p("package Foo { our $x = 1; }");
}

#[test]
fn accepts_unit_underscore_file_test() {
    p("-e $0");
}

#[test]
fn accepts_stat_underscore_on_filehandle() {
    p("stat STDIN");
}

#[test]
fn accepts_chomp_array() {
    p("chomp @lines");
}

#[test]
fn accepts_map_block_with_comma_separator() {
    p("map { $_ * 2 } 1, 2, 3");
}

#[test]
fn accepts_sort_subroutine_block() {
    p("sort { $a cmp $b } qw(b a)");
}

#[test]
fn accepts_grep_block_with_regex_inside() {
    p("grep { /^a/ } @lines");
}

#[test]
fn accepts_split_limit_omit_pattern() {
    p("split ' ', $s, 2");
}

#[test]
fn accepts_join_empty_string() {
    p("join '', (1, 2)");
}

#[test]
fn accepts_reverse_scalar_context_keyword() {
    p("scalar reverse 'abc'");
}

#[test]
fn accepts_index_optional_position() {
    p("index $hay, $needle, 3");
}

#[test]
fn accepts_substr_four_arg() {
    p("substr $s, 0, 3, $repl");
}

#[test]
fn accepts_sprintf_percent_s() {
    p(r#"sprintf "%s %d", "x", 1"#);
}

#[test]
fn accepts_hex_underscore_separator() {
    p("0xFF_FF");
}

#[test]
fn accepts_binary_underscore_separator() {
    p("0b1010_0001");
}

#[test]
fn accepts_floating_point_underscore() {
    p("1_000.5");
}

#[test]
fn accepts_logical_xor_operator() {
    p("1 xor 0");
}

#[test]
fn accepts_cmp_chained_with_spaceship() {
    p("1 <=> 2 <=> 3");
}

#[test]
fn accepts_repeat_via_assign() {
    p("my $s = 'a'; $s = $s x 3");
}

#[test]
fn accepts_bit_shift_via_assign() {
    p("my $n = 8; $n = $n >> 1");
}

#[test]
fn accepts_bit_and_via_assign() {
    p("my $n = 0xF; $n = $n & 0x3");
}

#[test]
fn accepts_or_assign_expanded() {
    p("my $v; $v = $v || 7");
}

#[test]
fn accepts_and_assign_expanded() {
    p("my $v = 1; $v = $v && 2");
}

#[test]
fn accepts_concat_assign_chain() {
    p(r#"my $a = "x"; $a .= "y" .= "z""#);
}
