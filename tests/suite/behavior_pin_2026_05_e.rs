//! Behavior-pinning batch E (2026-05-04): process control, heredocs, regex
//! character classes, sprintf flags, control flow forms (redo/do-while/until/
//! unless), case-folding, math builtins.

use crate::common::*;

// ── system() return value vs $? ──────────────────────────────────────────────

#[test]
fn system_true_returns_zero_in_both() {
    assert_eq!(eval_int(r#"system("true")"#), 0);
    assert_eq!(eval_int(r#"system("true"); $?"#), 0);
}

#[test]
fn system_false_returns_exit_code_not_status_word_today() {
    // BUG-030: Perl's `system()` returns the same value as `$?` (exit code in
    // the high byte). Stryke returns the bare exit code instead.
    assert_eq!(eval_int(r#"system("false")"#), 1);
    assert_eq!(eval_int(r#"system("false"); $?"#), 256);
}

#[test]
fn system_list_form_runs_without_shell() {
    // List form should not pass through a shell — verifying with a benign
    // command. We only assert that the call returns zero (the command
    // succeeded).
    assert_eq!(eval_int(r#"system("true", "")"#), 0);
}

#[test]
fn system_list_form_loses_exit_code_today() {
    // BUG-031: list-form `system("sh", "-c", "exit 7")` returns 0 and leaves
    // `$?` at 0; the single-string shell-quoted form correctly returns
    // 1792.
    assert_eq!(eval_int(r#"system("sh", "-c", "exit 7"); $?"#), 0);
}

#[test]
fn system_string_form_propagates_exit_code() {
    // The single-string form does propagate. Pin both this and the broken
    // list form so the asymmetry is visible.
    assert_eq!(
        eval_int(r#"system("sh -c \"exit 7\""); $?"#),
        1792
    );
}

// ── die with a blessed object: ref($@) returns class ────────────────────────

#[test]
fn die_with_blessed_object_preserves_class() {
    assert_eq!(
        eval_string(
            r#"package MyErr; sub new { bless { msg => $_[1] }, $_[0] }
               package main;
               eval { die MyErr->new("oops") };
               ref($@)"#
        ),
        "MyErr"
    );
}

#[test]
fn ref_dollar_at_eq_string_precedence_today() {
    // BUG-032: `ref $@ eq "MyErr"` parses as `ref ($@ eq "MyErr")` (named-unary
    // arg eats the binary expression). Pin: with parens it works, without
    // them it does not.
    let with_parens = eval_string(
        r#"package MyErr; sub new { bless {}, $_[0] }
           package main;
           eval { die MyErr->new };
           ((ref $@) eq "MyErr") ? "Y" : "N""#,
    );
    assert_eq!(with_parens, "Y");

    let without_parens = eval_string(
        r#"package MyErr; sub new { bless {}, $_[0] }
           package main;
           eval { die MyErr->new };
           (ref $@ eq "MyErr") ? "Y" : "N""#,
    );
    assert_eq!(without_parens, "N");
}

// ── Nested eval propagation ──────────────────────────────────────────────────

#[test]
fn nested_eval_inner_does_not_leak_to_outer() {
    let out = eval_string(
        r#"my $log = "";
           eval {
             eval { die "inner\n" };
             $log .= "in:$@";
             die "outer\n";
           };
           $log .= "out:$@";
           $log"#,
    );
    assert_eq!(out, "in:inner\nout:outer\n");
}

// ── $& not interpolated in s/// replacement string today ─────────────────────

#[test]
fn dollar_amp_not_interpolated_in_replacement_today() {
    // BUG-033: `s/(\d+)/$&/g` should expand `$&` to the matched substring.
    // Stryke leaves `$&` literal in the replacement.
    assert_eq!(
        eval_string(r#"my $s = "abc 123"; $s =~ s/(\d+)/$&/g; $s"#),
        "abc $&"
    );
}

#[test]
fn captures_dollar_one_dollar_two_work_in_replacement() {
    // The numbered-capture form does interpolate, so the issue is specific
    // to `$&`.
    assert_eq!(
        eval_string(r#"my $s = "abc 123"; $s =~ s/(\d)(\d)(\d)/<$3$2$1>/r"#),
        "abc <321>"
    );
}

// ── Heredoc forms ────────────────────────────────────────────────────────────

#[test]
fn heredoc_indented_strips_leading_indent() {
    assert_eq!(
        eval_string(
            "my $x = <<~END;\n    line1\n    line2\n    END\n$x"
        ),
        "line1\nline2\n"
    );
}

#[test]
fn heredoc_single_quoted_does_not_interpolate() {
    let out = eval_string(
        "my $name = \"world\"; my $x = <<'END';\nhello $name\nEND\n$x",
    );
    assert_eq!(out, "hello $name\n");
}

#[test]
fn heredoc_double_quoted_interpolates() {
    let out = eval_string(
        "my $name = \"world\"; my $x = <<\"END\";\nhello $name\nEND\n$x",
    );
    assert_eq!(out, "hello world\n");
}

#[test]
fn multiple_heredocs_on_same_line_not_supported_today() {
    // BUG-034: `print <<A, <<B;` should accept two terminators on one line,
    // each consuming its own body. Stryke parses the second body as code.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind("print <<A, <<B;\nA1\nA\nB1\nB\n");
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type | ErrorKind::Syntax | ErrorKind::UndefinedSubroutine),
        "expected error, got {:?}",
        kind
    );
}

// ── q / qq / qr / qw ─────────────────────────────────────────────────────────

#[test]
fn q_does_not_interpolate() {
    assert_eq!(eval_string(r#"q{single $foo}"#), "single $foo");
}

#[test]
fn qq_interpolates() {
    assert_eq!(eval_string(r#"my $x = "var"; qq{double $x}"#), "double var");
}

#[test]
fn qw_returns_list() {
    assert_eq!(
        eval_string(r#"my @a = qw(a b c); "@a""#),
        "a b c"
    );
}

#[test]
fn qr_creates_compiled_regex_with_modifier() {
    assert_eq!(
        eval_int(r#"my $r = qr/abc/i; "ABC" =~ $r ? 1 : 0"#),
        1
    );
}

// ── sprintf format flags (working and broken) ────────────────────────────────

#[test]
fn sprintf_capital_x_uppercase_hex() {
    assert_eq!(eval_string(r#"sprintf("%X", 255)"#), "FF");
}

#[test]
fn sprintf_hash_flag_does_not_add_prefix_today() {
    // BUG-035: `%#x` should produce `0xff`; stryke produces `ff`. Same for
    // `%#o` (Perl: `010`, stryke: `10`).
    assert_eq!(eval_string(r#"sprintf("%#x", 255)"#), "ff");
    assert_eq!(eval_string(r#"sprintf("%#o", 8)"#), "10");
}

#[test]
fn sprintf_u_renders_negative_one_as_max_unsigned() {
    assert_eq!(
        eval_string(r#"sprintf("%u", -1)"#),
        "18446744073709551615"
    );
}

// ── Math builtins ────────────────────────────────────────────────────────────

#[test]
fn floor_rounds_toward_negative_infinity() {
    assert_eq!(eval_int("floor(3.5)"), 3);
    assert_eq!(eval_int("floor(-3.5)"), -4);
}

#[test]
fn ceil_rounds_toward_positive_infinity() {
    assert_eq!(eval_int("ceil(3.1)"), 4);
    assert_eq!(eval_int("ceil(-3.1)"), -3);
}

#[test]
fn round_uses_round_half_up() {
    assert_eq!(eval_int("round(3.5)"), 4);
    assert_eq!(eval_int("round(2.5)"), 3);
    assert_eq!(eval_int("round(3.4)"), 3);
}

// ── POSIX-style regex character classes ─────────────────────────────────────

#[test]
fn posix_alpha_class_matches_letters() {
    assert_eq!(
        eval_int(r#""abc123" =~ /[[:alpha:]]+/ ? 1 : 0"#),
        1
    );
}

#[test]
fn posix_digit_class_matches_digits() {
    assert_eq!(
        eval_int(r#""abc123" =~ /[[:digit:]]+/ ? 1 : 0"#),
        1
    );
}

#[test]
fn posix_space_class_matches_whitespace() {
    assert_eq!(
        eval_int(r#""abc 123" =~ /[[:space:]]/ ? 1 : 0"#),
        1
    );
}

#[test]
fn posix_lower_class_anchored_match() {
    assert_eq!(
        eval_int(r#""abc" =~ /^[[:lower:]]+$/ ? 1 : 0"#),
        1
    );
    assert_eq!(
        eval_int(r#""ABC" =~ /^[[:lower:]]+$/ ? 1 : 0"#),
        0
    );
}

#[test]
fn unicode_property_p_greek() {
    assert_eq!(eval_int(r#""α" =~ /\p{Greek}/ ? 1 : 0"#), 1);
}

#[test]
fn unicode_property_p_latin() {
    assert_eq!(eval_int(r#""abc" =~ /\p{Latin}/ ? 1 : 0"#), 1);
}

// ── split with explicit limit ────────────────────────────────────────────────

#[test]
fn split_default_strips_trailing_empties() {
    assert_eq!(
        eval_int(r#"my @p = split /:/, "foo:bar:baz:"; scalar @p"#),
        3
    );
}

#[test]
fn split_negative_limit_preserves_trailing_empties() {
    assert_eq!(
        eval_int(r#"my @p = split /:/, "foo:bar:baz:", -1; scalar @p"#),
        4
    );
}

#[test]
fn split_positive_limit_caps_count() {
    let out = eval_string(r#"my @p = split /:/, "a:b:c:d", 2; "@p""#);
    assert_eq!(out, "a b:c:d");
}

// ── Numeric coercion ─────────────────────────────────────────────────────────

#[test]
fn numeric_scientific_with_decimal() {
    assert_eq!(eval_int(r#""5.5e2" + 0"#), 550);
}

#[test]
fn numeric_string_with_leading_plus() {
    assert_eq!(eval_int(r#""+5" + 0"#), 5);
}

#[test]
fn numeric_string_with_leading_minus() {
    assert_eq!(eval_int(r#""-5" + 0"#), -5);
}

// ── Loop forms: redo / do-while / until / unless ────────────────────────────

#[test]
fn redo_re_runs_iteration_without_advancing() {
    let out = eval_string(
        r#"my $log = ""; my $count = 0;
           for my $i (1..3) {
             if ($i == 2 && $count < 2) { $count++; redo }
             $log .= "$i,";
           }
           $log"#,
    );
    // i=1 once, i=2 retried twice (so seen 3 times in total), i=3 once.
    assert_eq!(out, "1,2,3,");
}

#[test]
fn do_while_loop_runs_at_least_once() {
    assert_eq!(
        eval_string(r#"my $i = 0; my $s = ""; do { $s .= $i; $i++ } while ($i < 3); $s"#),
        "012"
    );
}

#[test]
fn until_loop_runs_until_condition_true() {
    assert_eq!(
        eval_string(r#"my $i = 0; my $s = ""; until ($i >= 3) { $s .= $i; $i++ } $s"#),
        "012"
    );
}

#[test]
fn unless_block_runs_when_false() {
    assert_eq!(
        eval_string(r#"my $r; unless (0) { $r = "Y" } else { $r = "N" } $r"#),
        "Y"
    );
}

#[test]
fn unless_statement_modifier() {
    assert_eq!(
        eval_string(r#"my $r = "no"; $r = "yes" unless 0; $r"#),
        "yes"
    );
}

// ── fc() case-folding ────────────────────────────────────────────────────────

#[test]
fn fc_makes_string_eq_match_case_insensitive() {
    assert_eq!(eval_int(r#"fc("abc") eq fc("ABC") ? 1 : 0"#), 1);
}

// ── exists vs defined on array ──────────────────────────────────────────────

#[test]
fn array_index_with_undef_value_exists_but_not_defined() {
    assert_eq!(
        eval_string(
            r#"my @a = (1, undef, 3);
               my $e = exists $a[1] ? "Y" : "N";
               my $d = defined $a[1] ? "Y" : "N";
               "$e/$d""#
        ),
        "Y/N"
    );
}

#[test]
fn array_index_out_of_bounds_does_not_exist() {
    assert_eq!(
        eval_int(r#"my @a = (1, 2); exists $a[5] ? 1 : 0"#),
        0
    );
}

// ── Nested string repetition (x in list context) ────────────────────────────

#[test]
fn list_x_with_one_element_creates_array_of_repeated_value() {
    assert_eq!(
        eval_string(r#"my $x = "aaa"; my @a = ($x) x 3; "@a""#),
        "aaa aaa aaa"
    );
}

// ── pipe open (read & write) ─────────────────────────────────────────────────

#[test]
fn pipe_open_read_string_form_captures_subprocess_stdout() {
    // The single-string shell form works.
    assert_eq!(
        eval_string(
            r#"open my $fh, "-|", "echo hi" or die;
               my $l = <$fh>;
               close $fh;
               chomp $l;
               $l"#
        ),
        "hi"
    );
}

#[test]
fn pipe_open_read_list_form_drops_args_today() {
    // BUG-036: `open my $fh, "-|", "echo", "hi"` ignores the extra argument
    // and runs `echo` with no args (so reads just "\n"). Perl runs `echo hi`
    // without involving a shell.
    assert_eq!(
        eval_string(
            r#"open my $fh, "-|", "echo", "hi" or die;
               my $l = <$fh>;
               close $fh;
               $l"#
        ),
        "\n"
    );
}

#[test]
fn pipe_open_write_form_succeeds() {
    // We can't capture the child's stdout from inside `eval_string` (the test
    // process owns it), so just verify the open + close cycle succeeds.
    let out = eval_string(
        r#"open my $fh, "|-", "cat" or die;
           print $fh "via pipe\n";
           close $fh;
           "OK""#,
    );
    assert_eq!(out, "OK");
}

// ── Backref reorder via captures ─────────────────────────────────────────────

#[test]
fn match_three_captures_reorder() {
    assert_eq!(
        eval_string(r#"my $s = "hello"; $s =~ /(.)(.)(.)/; "$3$2$1""#),
        "leh"
    );
}

// ── Sort default behaviour for numeric-looking strings ──────────────────────

#[test]
fn sort_default_is_lexicographic_not_numeric() {
    // 30 < 5 lexicographically.
    assert_eq!(
        eval_string(r#"join(",", sort 5, 30, 7)"#),
        "30,5,7"
    );
}

#[test]
fn sort_numeric_comparator_orders_correctly() {
    assert_eq!(
        eval_string(r#"join(",", sort { $a <=> $b } 5, 30, 7)"#),
        "5,7,30"
    );
}
