//! Pin `#{ EXPR }` Ruby-style expression interpolation inside
//! double-quoted strings — the form `docs/STYLE_GUIDE.md` §1a says
//! to use instead of `.` concatenation. Probed against the running
//! interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn hash_brace_arithmetic() {
    let code = r##"
        my $x = 5;
        my $s = "sq=#{$x * $x}";
        $s eq "sq=25" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_builtin_call() {
    let code = r##"
        my @a = (1, 2, 3, 4);
        my $s = "sum=#{sum @a}";
        $s eq "sum=10" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_len_of_string() {
    let code = r##"
        my $x = "hello";
        my $s = "len=#{len $x}";
        $s eq "len=5" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_function_call_chain() {
    let code = r##"
        my $name = "world";
        my $s = "hello, #{uc $name}!";
        $s eq "hello, WORLD!" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_compound_expression() {
    let code = r##"
        my $i = 3;
        my $s = "page #{2 * $i + 1} of 10";
        $s eq "page 7 of 10" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_user_defined_function() {
    let code = r##"
        fn Demo::Hbi::twice($n) = $n * 2;
        my $s = "doubled=#{Demo::Hbi::twice(21)}";
        $s eq "doubled=42" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_inside_p_output() {
    // The idiom from style guide §1a — `p "sum is #{sum @nums}"`.
    let code = r##"
        my @nums = (10, 20, 30);
        my $captured = "sum is #{sum @nums}";
        $captured eq "sum is 60" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_multiple_per_string() {
    let code = r##"
        my $a = 7;
        my $b = 3;
        my $s = "#{$a} + #{$b} = #{$a + $b}";
        $s eq "7 + 3 = 10" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_no_interp_in_single_quotes() {
    // Single-quoted strings never interpolate — `#{}` stays literal.
    let code = r##"
        my $x = 5;
        my $s = 'sq=#{$x * $x}';
        $s eq 'sq=#{$x * $x}' ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_ternary_expression() {
    let code = r##"
        my $n = 7;
        my $s = "n is #{$n % 2 == 0 ? 'even' : 'odd'}";
        $s eq "n is odd" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_array_slice_expression() {
    let code = r##"
        my @a = (10, 20, 30, 40, 50);
        my $s = "last=#{$a[-1]}";
        $s eq "last=50" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_hash_value_lookup() {
    let code = r##"
        my %h = (a => 1, b => 2, c => 3);
        my $s = "b=#{$h{b}}";
        $s eq "b=2" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_with_pipe_forward_chain() {
    // Per style guide §6b: pipeline reads in execution order.
    let code = r##"
        my $w = "hello";
        my $s = "rev=#{$w |> rev}";
        $s eq "rev=olleh" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

// ── Nested quotes inside interpolation regions (fixed 2026-06-11) ─────
// The lexer used to end the outer `"…"` at the first `"` inside
// `#{…}` / `@{…}` / `${…}`, forcing the `'…'` workaround. The string
// scanner now tracks interp regions quote-aware, so these pin the
// direct forms.

#[test]
fn hash_brace_nested_double_quoted_literal() {
    let code = r##"
        my $s = "ok #{"tom"}";
        $s eq "ok tom" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_empty_double_quoted_literal() {
    let code = r##"
        my $s = "#{""}";
        $s eq "" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_closing_brace_inside_nested_quotes() {
    // A `}` inside the nested `"…"` must not close the `#{…}` region.
    let code = r##"
        my $s = "brace #{"}"}";
        $s eq "brace }" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_ternary_with_nested_double_quotes() {
    let code = r##"
        my $n = 7;
        my $s = "n is #{$n % 2 == 0 ? "even" : "odd"}";
        $s eq "n is odd" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_pipe_forward_on_nested_string_literal() {
    let code = r##"
        my $s = "rev=#{"hello" |> rev}";
        $s eq "rev=olleh" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn at_brace_anon_array_with_nested_double_quotes() {
    let code = r##"
        my $s = "ok @{["tom"]}";
        $s eq "ok tom" ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hash_brace_escaped_quote_form_still_works() {
    // The pre-fix `\"` spelling stays valid.
    let code = "my $s = \"got #{length(\\\"x\\\")}\";\n$s eq \"got 1\" ? 1 : 0";
    assert_eq!(eval_int(code), 1);
}

#[test]
fn qq_and_backticks_track_interp_regions() {
    let code = r##"
        my $a = qq/ok #{"tom"}/;
        my $b = `echo #{"hi"}`;
        chomp $b;
        ($a eq "ok tom" && $b eq "hi") ? 1 : 0
    "##;
    assert_eq!(eval_int(code), 1);
}
