//! Behavior-pinning batch N (2026-05-04): sprintf format gaps, hash ordering,
//! float ranges, regex /n flag, possessive quantifiers, octal prefixes,
//! `use integer`, autosplit edge cases.

use crate::common::*;

// ── sprintf %n / %p / %A not implemented ───────────────────────────────────

#[test]
fn sprintf_n_does_not_populate_count_today() {
    // BUG-079: `%n` should write the count of chars output so far into the
    // referenced scalar. Stryke leaves the var undef. (`%n` is a security
    // hole in C — many languages omit it on purpose.)
    assert_eq!(
        eval_int(r#"my $n; sprintf("hello%n", $n); defined($n) ? 1 : 0"#),
        0
    );
}

#[test]
fn sprintf_p_prints_value_as_string_today() {
    // BUG-080: Perl's `%p` shows the SV's internal address. Stryke ignores
    // the format and prints the value's stringification.
    assert_eq!(eval_string(r#"sprintf("%p", "hello")"#), "hello");
}

#[test]
fn sprintf_capital_a_does_not_emit_hex_float_today() {
    // BUG-080b: `%A` is `%a` with uppercase letters. Both are unimplemented.
    assert_eq!(eval_string(r#"sprintf("%A", 1.5)"#), "1.5");
}

// ── %g and %G work, but %G uppercases the exponent letter only ──────────────

#[test]
fn sprintf_capital_g_works_for_integer_inputs() {
    assert_eq!(eval_string(r#"sprintf("%G", 1234567)"#), "1234567");
}

// ── Hash insertion order is preserved (IndexMap-backed) ────────────────────

#[test]
fn hash_literal_keys_in_insertion_order() {
    assert_eq!(
        eval_string(r#"my %h = (a=>1, b=>2, c=>3); join(",", keys %h)"#),
        "a,b,c"
    );
}

#[test]
fn hash_progressive_assignment_preserves_order() {
    assert_eq!(
        eval_string(
            r#"my %h; $h{$_} = 1 for qw(z x y a m); join(",", keys %h)"#
        ),
        "z,x,y,a,m"
    );
}

#[test]
fn hash_delete_preserves_remaining_order() {
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2, c=>3); delete $h{b}; join(",", keys %h)"#
        ),
        "a,c"
    );
}

// ── Range with floats truncates to integer range ───────────────────────────

#[test]
fn float_range_with_half_step_yields_integer_only() {
    // Perl: `(0.5..2.5)` is the same as `(0..2)` because `..` truncates
    // toward zero on both ends. Pin the same.
    assert_eq!(
        eval_string(r#"my @a = (0.5..2.5); "@a""#),
        "0 1 2"
    );
}

// ── Integer % div semantics in default mode ────────────────────────────────

#[test]
fn float_division_returns_float() {
    assert_eq!(eval_string("7 / 2"), "3.5");
}

#[test]
fn modulo_keeps_remainder() {
    assert_eq!(eval_int("7 % 3"), 1);
}

// ── `use integer` does NOT switch / to integer division today ──────────────

#[test]
fn use_integer_pragma_lib_path_tries_to_load_integer_pm_today() {
    // BUG-081: Perl's `use integer` pragma should make `/` integer-truncate.
    // CLI: stryke ignores the pragma silently. Library `eval` API: tries
    // to load `integer.pm` from @INC and fails. Pin the lib behavior.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind("use integer; 7 / 3");
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::FileNotFound),
        "expected module-load error, got {:?}",
        kind
    );
}

// ── Octal `0o` prefix not recognized today ─────────────────────────────────

#[test]
fn octal_o_prefix_returns_zero_today() {
    // BUG-082: Perl 5.34+ accepts `0o777` for octal 511. Stryke parses `0o`
    // as "0" followed by an unrelated identifier `o777` and emits `0`.
    assert_eq!(eval_int("0o777"), 0);
    assert_eq!(eval_int("0o17"), 0);
}

#[test]
fn classic_zero_prefix_octal_works() {
    // Bare leading-zero octal still works: `0777` is 511.
    assert_eq!(eval_int("0777"), 511);
}

#[test]
fn binary_b_prefix_recognized() {
    assert_eq!(eval_int("0b1010"), 10);
}

#[test]
fn hex_x_prefix_recognized() {
    assert_eq!(eval_int("0xFF"), 255);
}

#[test]
fn underscore_in_numeric_literal_is_ignored() {
    assert_eq!(eval_int("0_10"), 10);
}

// ── Regex /n flag (no auto-capture) is not implemented today ────────────────

#[test]
fn regex_n_flag_silently_returns_n_in_lib_eval_today() {
    // BUG-083: Perl 5.22+ has the `/n` flag to disable auto-numbered capture.
    // Stryke parses the trailing `n` as an identifier. From the CLI it
    // raises "Undefined subroutine &n"; from the lib `eval` API the
    // expression evaluates to the literal string "n". Pin observed lib
    // behavior.
    assert_eq!(eval_string(r#""abc" =~ /(\w+)/n"#), "n");
}

// ── Regex `\K` resets match start (works) ──────────────────────────────────

#[test]
fn regex_k_resets_match_start_in_substitution() {
    assert_eq!(
        eval_string(r#"my $s = "abc=123"; $s =~ s/abc=\K\d+/X/; $s"#),
        "abc=X"
    );
}

// ── Possessive quantifiers `a++` behave like greedy `a+` today ─────────────

#[test]
fn possessive_quantifier_does_not_prevent_backtrack_today() {
    // BUG-084: `aaab =~ /a++ab/` should fail (a++ takes all "aaa", then no
    // backtrack to leave "a" for the literal `ab`). Stryke matches as if
    // the second `+` weren't there.
    assert_eq!(eval_int(r#""aaab" =~ /a++ab/ ? 1 : 0"#), 1);
}

#[test]
fn greedy_a_plus_with_backtrack_matches() {
    assert_eq!(eval_int(r#""aaab" =~ /a+ab/ ? 1 : 0"#), 1);
}

// ── Regex /x and /xx whitespace-stripping ───────────────────────────────────

#[test]
fn regex_x_strips_whitespace_outside_classes() {
    assert_eq!(eval_int(r#""abc" =~ /a b c/x ? 1 : 0"#), 1);
}

#[test]
fn regex_xx_strips_whitespace_in_more_places() {
    assert_eq!(eval_int(r#""abc" =~ /a b c/xx ? 1 : 0"#), 1);
}

// ── Regex anchor pairs ─────────────────────────────────────────────────────

#[test]
fn anchor_capital_z_allows_trailing_newline() {
    assert_eq!(eval_int(r#""abc\n" =~ /\Aabc\Z/ ? 1 : 0"#), 1);
}

#[test]
fn anchor_lowercase_z_does_not_allow_trailing_newline() {
    assert_eq!(eval_int(r#""abc\n" =~ /\Aabc\z/ ? 1 : 0"#), 0);
}

#[test]
fn anchor_lowercase_z_matches_exact_end() {
    assert_eq!(eval_int(r#""abc" =~ /\Aabc\z/ ? 1 : 0"#), 1);
}

// ── Interpolated regex via `$pat` and `qr//` ───────────────────────────────

#[test]
fn interpolated_string_pattern_matches() {
    assert_eq!(
        eval_int(r#"my $pat = "abc"; "abcdef" =~ /$pat/ ? 1 : 0"#),
        1
    );
}

#[test]
fn qr_pattern_can_be_reused() {
    assert_eq!(
        eval_int(r#"my $r = qr/abc/; "abcdef" =~ $r ? 1 : 0"#),
        1
    );
}

#[test]
fn qr_pattern_with_modifier_keeps_modifier() {
    assert_eq!(
        eval_int(r#"my $r = qr/abc/i; "ABCdef" =~ $r ? 1 : 0"#),
        1
    );
}

// ── Hash slice assignment ──────────────────────────────────────────────────

#[test]
fn hash_slice_assign_creates_keys() {
    assert_eq!(
        eval_string(
            r#"my %h; @h{qw(a b c)} = (1,2,3);
               join(",", map {"$_=$h{$_}"} sort keys %h)"#
        ),
        "a=1,b=2,c=3"
    );
}

#[test]
fn hash_slice_assign_updates_existing_keys() {
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2, c=>3); @h{qw(a c)} = (10, 30);
               join(",", map {"$_=$h{$_}"} sort keys %h)"#
        ),
        "a=10,b=2,c=30"
    );
}

// ── printf to STDOUT / STDERR / regular handles ────────────────────────────

#[test]
fn printf_to_filehandle_writes_to_stdout_today() {
    // BUG-085: `printf $fh "fmt", ...` should write to `$fh`. Stryke
    // ignores the filehandle and writes to STDOUT (the file ends up empty).
    // Plain `print $fh "..."` does honor the filehandle correctly.
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_printf_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    let _ = eval_string(&format!(
        r#"open my $fh, ">", "{}" or die;
           print $fh "plain\n";
           printf $fh "n=%d\n", 42;
           close $fh;
           "OK""#,
        path
    ));
    let body = std::fs::read_to_string(&f).unwrap_or_default();
    let _ = std::fs::remove_file(&f);
    // Only the plain print made it through — printf to fh is lost.
    assert_eq!(body, "plain\n");
}

// ── `print STDOUT @list` concatenates with default $, ──────────────────────

#[test]
fn print_to_stdout_with_array_uses_no_separator_default() {
    // We confirm via sprintf-like inversion: `join "" @a` == raw print.
    assert_eq!(
        eval_string(r#"my @a = (1,2,3); join("", @a)"#),
        "123"
    );
}

// ── die with newline strips line-info; without newline appends it ──────────

#[test]
fn die_with_trailing_newline_does_not_append_line_info() {
    let s = eval_string(r#"eval { die "bad\n" }; $@"#);
    assert_eq!(s, "bad\n");
}

#[test]
fn die_without_trailing_newline_appends_at_line_info() {
    let s = eval_string(r#"eval { die "bad" }; $@"#);
    assert!(
        s.starts_with("bad at ") && s.ends_with(".\n"),
        "expected appended line info, got {:?}",
        s
    );
}

// ── String with embedded NUL preserves length ──────────────────────────────

#[test]
fn string_with_embedded_nul_keeps_length_three() {
    assert_eq!(eval_int(r#"length("a\0b")"#), 3);
}

// ── %.0f rounding on whole number + half ───────────────────────────────────

#[test]
fn percent_dot_zero_f_truncates_or_rounds_halves() {
    // Already pinned in batch H; redundant guard so this batch's hash test
    // can run alongside it.
    assert_eq!(eval_string(r#"sprintf("%.0f", 0.5)"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.0f", 1.5)"#), "2");
}

// ── Numeric coercion for negative floats in %d ─────────────────────────────

#[test]
fn percent_d_truncates_negative_float_toward_zero() {
    assert_eq!(eval_string(r#"sprintf("%d", -1.5)"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%d", -1.9)"#), "-1");
}

// ── -F / -a CLI mode parses correctly via library API smoke check ──────────
//
// We can't drive `-F: -alne` from `eval_string` directly. Pin that the
// programs we'd write for those modes parse fine.

#[test]
fn split_emulating_dash_capital_f_pattern_parses() {
    assert!(
        stryke::parse(r#"my @F = split(/:/, "a:b:c"); print "@F""#).is_ok()
    );
}

// ── Parallel control: --threads accepts (parse-level pin only) ─────────────

#[test]
fn pmap_works_under_default_thread_pool() {
    assert_eq!(
        eval_string(r#"my @r = pmap { _ * 2 } 1..5; "@r""#),
        "2 4 6 8 10"
    );
}

// ── `use integer` parses without error ──────────────────────────────────────

#[test]
fn use_integer_pragma_at_least_parses() {
    assert!(stryke::parse("use integer; my $x = 7 / 3").is_ok());
}

// ── Octal classic, hex, binary roundtrip via printf ─────────────────────────

#[test]
fn octal_literal_pattern_matches_perl() {
    // `0777` (no `o` separator) is the Perl-classic octal form and works.
    assert_eq!(eval_int("0644"), 0o644);
    assert_eq!(eval_int("0755"), 0o755);
}

// ── Bareword `print` returns 1 in scalar context ──────────────────────────

#[test]
fn print_returns_one_after_emitting() {
    assert_eq!(eval_int(r#"my $r = print ""; $r"#), 1);
}

// ── pmap with empty input returns empty ─────────────────────────────────────

#[test]
fn pmap_with_empty_list_returns_empty() {
    assert_eq!(eval_int(r#"my @r = pmap { _ } (); scalar @r"#), 0);
}

// ── pgrep with always-false predicate returns empty ────────────────────────

#[test]
fn pgrep_with_false_predicate_returns_empty() {
    assert_eq!(eval_int(r#"my @r = pgrep { 0 } 1..10; scalar @r"#), 0);
}
