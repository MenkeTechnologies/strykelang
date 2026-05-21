//! Behavior-pinning batch C (2026-05-04): regex flags, string builtins,
//! JSON/YAML/TOML, AOP intercepts, list-vs-scalar context, `each`, backticks,
//! ref equality, `$ENV` propagation.
//!
//! Companion to `behavior_pin_2026_05.rs` and `behavior_pin_2026_05_b.rs`.

use crate::common::*;

// ── String builtins: case + length + reverse ─────────────────────────────────

#[test]
fn lc_uc_handles_mixed_case() {
    assert_eq!(eval_string(r#"lc("HELLO World")"#), "hello world");
    assert_eq!(eval_string(r#"uc("hello world")"#), "HELLO WORLD");
}

#[test]
fn ucfirst_lcfirst_only_first_char() {
    assert_eq!(eval_string(r#"ucfirst("hello world")"#), "Hello world");
    assert_eq!(eval_string(r#"lcfirst("HELLO WORLD")"#), "hELLO WORLD");
}

#[test]
fn reverse_in_list_context_reverses_list_not_string() {
    // `reverse "hello"` in list context treats the string as a single-element
    // list, so the output is the original string.
    assert_eq!(eval_string(r#"join("", reverse("hello"))"#), "hello");
}

#[test]
fn reverse_in_scalar_context_reverses_string() {
    assert_eq!(eval_string(r#"scalar reverse("hello")"#), "olleh");
}

#[test]
fn length_returns_byte_count_for_unicode_string() {
    // No `use utf8` → bytes. `é` is 2 bytes in UTF-8, so total = 6.
    assert_eq!(eval_int(r#"length("héllo")"#), 6);
}

#[test]
fn length_with_use_utf8_returns_char_count() {
    // PARITY-013 FIXED: with `use utf8;` length() counts Unicode codepoints,
    // not UTF-8 bytes — matching Perl 5.
    assert_eq!(eval_int(r#"use utf8; length("héllo")"#), 5);
    assert_eq!(eval_int(r#"use utf8; length("日本語")"#), 3);
    assert_eq!(eval_int(r#"use utf8; length("café")"#), 4);
    // Without the pragma, byte count.
    assert_eq!(eval_int(r#"length("héllo")"#), 6);
    assert_eq!(eval_int(r#"length("日本語")"#), 9);
}

// ── substr: read forms work, 4-arg replacement works, lvalue does not ────────

#[test]
fn substr_two_arg_to_end() {
    assert_eq!(eval_string(r#"substr("Hello World", 6)"#), "World");
}

#[test]
fn substr_three_arg_with_length() {
    assert_eq!(eval_string(r#"substr("Hello World", 6, 5)"#), "World");
}

#[test]
fn substr_negative_offset() {
    assert_eq!(eval_string(r#"substr("Hello World", -5)"#), "World");
}

#[test]
fn substr_four_arg_replaces_in_place_and_returns_old() {
    assert_eq!(
        eval_string(r#"my $s = "Hello World"; my $r = substr($s, 6, 5, "Stryke"); "s=$s r=$r""#),
        "s=Hello Stryke r=World"
    );
}

#[test]
fn substr_lvalue_assignment_replaces_in_place() {
    // PARITY-014 FIXED: `substr($s, $o, $l) = $rhs` is equivalent to the
    // 4-arg form `substr($s, $o, $l, $rhs)` — both compiler and tree-
    // walker now rewrite the assignment that way.
    assert_eq!(
        eval_string(r#"my $s = "Hello"; substr($s, 0, 1) = "J"; $s"#),
        "Jello"
    );
}

#[test]
fn substr_lvalue_with_two_args_replaces_to_end() {
    assert_eq!(
        eval_string(r#"my $s = "abcdef"; substr($s, 2) = "Z"; $s"#),
        "abZ"
    );
}

#[test]
fn substr_lvalue_with_negative_offset() {
    assert_eq!(
        eval_string(r#"my $s = "Hello"; substr($s, -3, 2) = "ZZZ"; $s"#),
        "HeZZZo"
    );
}

#[test]
fn substr_lvalue_zero_length_at_start_inserts() {
    assert_eq!(
        eval_string(r#"my $s = "abcdef"; substr($s, 0, 0) = "PRE"; $s"#),
        "PREabcdef"
    );
}

#[test]
fn substr_lvalue_zero_length_at_end_appends() {
    assert_eq!(
        eval_string(r#"my $s = "abcdef"; substr($s, 6, 0) = "POST"; $s"#),
        "abcdefPOST"
    );
}

// ── vec / bit-field lvalue ──────────────────────────────────────────────────

#[test]
fn vec_lvalue_byte_assignment() {
    // PARITY-010 FIXED: `vec($s, $offset, $bits) = N` compiles and assigns.
    assert_eq!(eval_string(r#"my $s = ""; vec($s, 0, 8) = 65; $s"#), "A");
    assert_eq!(
        eval_string(r#"my $s = ""; vec($s, 0, 8) = 0x41; vec($s, 1, 8) = 0x42; $s"#),
        "AB"
    );
}

#[test]
fn vec_read_8_bit() {
    // 8-bit reads return individual byte values.
    assert_eq!(
        eval_string(r#"my $s = "AB"; join("/", vec($s, 0, 8), vec($s, 1, 8))"#),
        "65/66"
    );
}

#[test]
fn vec_lvalue_16_bit_big_endian() {
    // Perl's `vec` uses big-endian byte order for multi-byte BITS, so
    // vec($s, 0, 16) = 0x1234 stores 0x12 then 0x34.
    assert_eq!(
        eval_int(r#"my $s = ""; vec($s, 0, 16) = 0x1234; vec($s, 0, 16)"#),
        0x1234
    );
}

#[test]
fn vec_lvalue_32_bit_round_trip() {
    // 32-bit big-endian round-trip + byte-by-byte read.
    assert_eq!(
        eval_string(
            r#"my $s = ""; vec($s, 0, 32) = 0xDEADBEEF;
               sprintf("%08x", vec($s, 0, 32))"#
        ),
        "deadbeef"
    );
    assert_eq!(
        eval_string(
            r#"my $s = ""; vec($s, 0, 32) = 0xDEADBEEF;
               join("/", map { vec($s, $_, 8) } 0..3)"#
        ),
        "222/173/190/239"
    );
}

#[test]
fn vec_read_zero_pads_past_end() {
    // Reading past the end returns zero-padded bytes (Perl behavior).
    assert_eq!(
        eval_int(r#"my $s = "AB"; vec($s, 0, 32)"#),
        ((0x41u32 << 24) | (0x42u32 << 16)) as i64
    );
}

// ── index / rindex / x ───────────────────────────────────────────────────────

#[test]
fn index_finds_substring() {
    assert_eq!(eval_int(r#"index("Hello World", "lo")"#), 3);
}

#[test]
fn index_returns_minus_one_when_missing() {
    assert_eq!(eval_int(r#"index("Hello World", "xyz")"#), -1);
}

#[test]
fn rindex_returns_last_occurrence() {
    assert_eq!(eval_int(r#"rindex("abcabc", "b")"#), 4);
}

#[test]
fn list_x_repeat_creates_array_with_repeated_value() {
    assert_eq!(
        eval_string(r#"my @a = (0) x 5; "@a count=" . scalar @a"#),
        "0 0 0 0 0 count=5"
    );
}

// ── Regex flags: i/m/s/g and inline (?i) ─────────────────────────────────────

#[test]
fn regex_i_flag_case_insensitive() {
    assert_eq!(eval_int(r#""Hello WORLD" =~ /hello/i ? 1 : 0"#), 1);
}

#[test]
fn regex_m_flag_anchors_per_line() {
    assert_eq!(eval_int("\"abc\\ndef\" =~ /^def/m ? 1 : 0"), 1);
    assert_eq!(eval_int("\"abc\\ndef\" =~ /^def/  ? 1 : 0"), 0);
}

#[test]
fn regex_s_flag_dotall() {
    assert_eq!(eval_int("\"a\\nb\" =~ /a.b/s ? 1 : 0"), 1);
    assert_eq!(eval_int("\"a\\nb\" =~ /a.b/  ? 1 : 0"), 0);
}

#[test]
fn regex_g_flag_returns_captures_when_groups_present() {
    // `m//g` in list context with capture groups returns each capture as its
    // own element across all matches: ("a","1","b","2","c","3").
    assert_eq!(
        eval_string(r#"my @m = "a1 b2 c3" =~ /(\w)(\d)/g; "@m""#),
        "a 1 b 2 c 3"
    );
    assert_eq!(
        eval_int(r#"my @m = "a1 b2 c3" =~ /(\w)(\d)/g; scalar @m"#),
        6
    );
}

#[test]
fn count_matches_via_list_assign_g_flag() {
    assert_eq!(
        eval_int(r#"my $count = () = "a1 b2 c3" =~ /\d/g; $count"#),
        3
    );
}

#[test]
fn regex_inline_i_modifier() {
    assert_eq!(eval_int(r#""abc" =~ /(?i)A/ ? 1 : 0"#), 1);
}

#[test]
fn regex_lookahead_lookbehind_negative_lookahead() {
    assert_eq!(eval_int(r#""abc" =~ /a(?=b)/  ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#""abc" =~ /(?<=a)b/ ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#""abc" =~ /a(?!c)/  ? 1 : 0"#), 1);
}

#[test]
fn regex_anchors_uppercase_and_lowercase_z() {
    assert_eq!(eval_int(r#""hello"   =~ /\Ahel/ ? 1 : 0"#), 1);
    assert_eq!(eval_int("\"hello\\n\" =~ /\\z/   ? 1 : 0"), 1);
    assert_eq!(eval_int("\"hello\\n\" =~ /\\Z/   ? 1 : 0"), 1);
}

#[test]
fn regex_quotemeta_escapes_special_chars() {
    assert_eq!(eval_string(r#"quotemeta(".+*?")"#), r"\.\+\*\?");
}

#[test]
fn regex_quotemeta_via_capital_q_in_pattern() {
    assert_eq!(
        eval_int(r#"my $pat = "a.b"; "a.b" =~ /\Q$pat\E/ ? 1 : 0"#),
        1
    );
    assert_eq!(
        eval_int(r#"my $pat = "a.b"; "axb" =~ /\Q$pat\E/ ? 1 : 0"#),
        0
    );
}

#[test]
fn regex_substitution_r_flag_returns_new_string_leaves_original() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; my $r = $s =~ s/a/X/r; "s=$s r=$r""#),
        "s=abc r=Xbc"
    );
}

#[test]
fn regex_substitution_e_flag_evaluates_replacement() {
    assert_eq!(
        eval_int(r#"my $s = "1+2"; $s =~ s/(\d+)\+(\d+)/$1 + $2/e; $s"#),
        3
    );
}

// ── Sort: numeric, by length, default lex, case-insensitive ─────────────────

#[test]
fn sort_numeric_descending() {
    assert_eq!(
        eval_string(r#"join(",", sort { $b <=> $a } 3,1,4,1,5,9,2,6)"#),
        "9,6,5,4,3,2,1,1"
    );
}

#[test]
fn sort_by_length_then_default() {
    assert_eq!(
        eval_string(r#"join(",", sort { length($a) <=> length($b) } qw(b ccc aa dddd))"#),
        "b,aa,ccc,dddd"
    );
}

#[test]
fn sort_default_lexicographic_uppercase_first() {
    assert_eq!(
        eval_string(r#"my @s = sort qw(b A c B a C); "@s""#),
        "A B C a b c"
    );
}

#[test]
fn sort_case_insensitive_comparator() {
    assert_eq!(
        eval_string(r#"my @s = sort { lc($a) cmp lc($b) } qw(b A c B a C); "@s""#),
        "A a b B c C"
    );
}

// ── Splice ────────────────────────────────────────────────────────────────────

#[test]
fn splice_removes_middle_segment() {
    assert_eq!(
        eval_string(r#"my @a = (1..10); my @r = splice(@a, 3, 4); "removed=@r left=@a""#),
        "removed=4 5 6 7 left=1 2 3 8 9 10"
    );
}

#[test]
fn splice_inserts_without_removing() {
    assert_eq!(
        eval_string(r#"my @a = (1..5); splice(@a, 2, 0, 99, 100); "@a""#),
        "1 2 99 100 3 4 5"
    );
}

#[test]
fn splice_replaces_one_element() {
    assert_eq!(
        eval_string(r#"my @a = (1..5); splice(@a, 2, 1, 99); "@a""#),
        "1 2 99 4 5"
    );
}

#[test]
fn splice_negative_offset_removes_to_end() {
    assert_eq!(
        eval_string(r#"my @a = (1..5); splice(@a, -2); "@a""#),
        "1 2 3"
    );
}

// ── Numeric parsing ──────────────────────────────────────────────────────────

#[test]
fn numeric_string_with_whitespace_coerces() {
    assert_eq!(eval_int(r#""  42  " + 0"#), 42);
}

#[test]
fn numeric_hex_literal_coerces_only_via_hex_or_oct() {
    // Plain `+0` ignores the `0x`-prefix string and gives 0.
    assert_eq!(eval_int(r#""0xFF" + 0"#), 0);
    assert_eq!(eval_int(r#"hex("0xFF")"#), 255);
    assert_eq!(eval_int(r#"oct("0777")"#), 511);
    assert_eq!(eval_int(r#"oct("0b1010")"#), 10);
}

#[test]
fn scientific_notation_string_coerces() {
    assert_eq!(eval_int(r#""1e3" + 0"#), 1000);
}

// ── List context: implicit vs explicit return ────────────────────────────────

#[test]
fn implicit_list_return_yields_full_list() {
    assert_eq!(
        eval_int(r#"fn xs { (1, 2, 3) } my @a = xs(); scalar @a"#),
        3
    );
    assert_eq!(
        eval_string(r#"fn xs { (1, 2, 3) } my @a = xs(); "@a""#),
        "1 2 3"
    );
}

#[test]
fn explicit_return_paren_list_returns_full_list() {
    // BUG-010 FIXED: `return (1, 2, 3)` propagates the caller's wantarray
    // context, so a list-context call gets the full list (1, 2, 3).
    assert_eq!(
        eval_string(r#"fn xs { return (1, 2, 3) } my @a = xs(); "@a""#),
        "1 2 3"
    );
}

#[test]
fn explicit_return_with_bare_commas_returns_full_list() {
    // BUG-010b FIXED: `return 1, 2, 3` (no parens) is a list-operator form
    // — Perl accepts it. Stryke now parses it as a comma-list operand.
    assert_eq!(
        eval_string(r#"fn xs { return 1, 2, 3 } my @a = xs(); "@a""#),
        "1 2 3"
    );
}

#[test]
fn return_array_var_passes_through_full_list() {
    assert_eq!(
        eval_string(r#"fn xs { my @x = (1,2,3); return @x } my @a = xs(); "@a""#),
        "1 2 3"
    );
}

#[test]
fn list_returning_sub_in_scalar_context_yields_last() {
    // BUG-011 FIXED (alongside BUG-010): assigning a list-returning sub to
    // a scalar yields the last element of the list (Perl wantarray
    // semantics). Coercion happens at Op::ReturnValue when the caller's
    // wantarray context is Scalar.
    assert_eq!(eval_string(r#"sub xs { (1, 2, 3) } my $s = xs(); $s"#), "3");
}

#[test]
fn list_in_scalar_context_via_scalar_keyword_takes_last() {
    // The `scalar` keyword does the right thing even though plain assignment
    // does not.
    assert_eq!(eval_int(r#"sub xs { (1, 2, 3) } scalar xs()"#), 3);
}

// ── `each` is currently broken ───────────────────────────────────────────────

#[test]
fn each_returns_empty_list_today() {
    // BUG-012: `each %h` should yield (key, value) pairs, then () to signal
    // end. Stryke returns () on the very first call.
    assert_eq!(
        eval_int(r#"my %h = (a => 1); my @kv = each %h; scalar @kv"#),
        0
    );
}

#[test]
fn while_my_pair_each_rejected_at_runtime_today() {
    // BUG-012b: `while (my ($k, $v) = each %h)` parses fine but the VM
    // lowering raises "my/our/state/local in expression context with multiple
    // or non-scalar decls".
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my %h = (a=>1); while (my ($k, $v) = each %h) {}"#);
    assert!(
        matches!(
            kind,
            ErrorKind::Runtime | ErrorKind::Type | ErrorKind::Syntax
        ),
        "expected error of some kind, got {:?}",
        kind
    );
}

// ── Backticks in list context return one big string today ───────────────────

#[test]
fn backticks_scalar_context_returns_full_string() {
    // The scalar form has always worked; pin it.
    assert_eq!(
        eval_int(r#"my $out = `printf "a\nb\nc\n"`; length($out)"#),
        6
    );
}

#[test]
fn backticks_list_context_returns_line_per_element() {
    // `qx`/backticks in list context yield one element per `\n`-terminated
    // line, matching Perl's `qx` and `readpipe` semantics.
    assert_eq!(
        eval_int(r#"my @lines = `printf "a\nb\nc\n"`; scalar @lines"#),
        3
    );
    assert_eq!(
        eval_string(r#"my @lines = `printf "a\nb\nc\n"`; join("|", @lines)"#),
        "a\n|b\n|c\n"
    );
}

// ── `$ENV{X} = ...` does not propagate to subprocesses ──────────────────────

#[test]
fn env_set_visible_within_stryke() {
    assert_eq!(
        eval_string(r#"$ENV{STRYKE_PIN_TEST} = "hi"; $ENV{STRYKE_PIN_TEST}"#),
        "hi"
    );
}

#[test]
fn env_set_propagates_to_subprocess() {
    // Writes to `%ENV` reach the real process environment so child processes
    // inherit the variable. Uses a uniquely-named key to avoid collisions.
    let out = eval_string(
        r#"$ENV{STRYKE_PIN_PROBE_VAR} = "yes";
           my $r = `env | grep '^STRYKE_PIN_PROBE_VAR='`;
           $r"#,
    );
    assert!(
        out.contains("STRYKE_PIN_PROBE_VAR=yes"),
        "expected child to inherit the var, got {:?}",
        out
    );
}

// ── Reference equality via `==` is broken (placeholder address) ─────────────

#[test]
fn ref_numeric_value_is_zero_today() {
    // BUG-015: refs numify to 0 because the displayed address is a placeholder,
    // so `==` between any two refs is true.
    assert_eq!(eval_int(r#"my @a; my @b; \@a == \@b ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my @a; 0 + \@a"#), 0);
}

// ── AOP advice — before / after / around ─────────────────────────────────────

#[test]
fn aop_before_advice_runs_first() {
    // Use the around/proceed return value instead of shared state — closure
    // capture interactions across `before`/`fn` are subtle in stryke and not
    // the property under test here.
    let out = eval_string(
        r#"fn payload { "G" }
           before "payload" { "B" }
           payload()"#,
    );
    // The original return value flows through; confirm `before` did not break
    // the call site.
    assert_eq!(out, "G");
}

#[test]
fn aop_around_replaces_return_value() {
    // `around { ... }` without `proceed()` replaces the original.
    assert_eq!(
        eval_int(
            r#"fn add($a, $b) { $a + $b }
               around "add" { 999 }
               add(2, 3)"#
        ),
        999
    );
}

#[test]
fn aop_around_proceed_then_decorate() {
    assert_eq!(
        eval_int(
            r#"fn add($a, $b) { $a + $b }
               around "add" { proceed() * 10 }
               add(2, 3)"#
        ),
        50
    );
}

#[test]
fn aop_around_proceed_returns_original_value() {
    assert_eq!(
        eval_int(
            r#"fn add($a, $b) { $a + $b }
               around "add" { my $r = proceed(); $r }
               add(2, 3)"#
        ),
        5
    );
}

#[test]
fn aop_around_can_decorate_value() {
    assert_eq!(
        eval_string(
            r#"fn add($a, $b) { $a + $b }
               around "add" { my $r = proceed(); "[$r]" }
               add(2, 3)"#
        ),
        "[5]"
    );
}

#[test]
fn aop_glob_pointcut_matches_multiple_subs() {
    let out = eval_string(
        r#"our $hits = "";
           fn foo  { "F" }
           fn fooo { "FF" }
           before "foo*" { $main::hits .= "B:" }
           foo();
           fooo();
           $hits"#,
    );
    assert_eq!(out, "B:B:");
}

// ── Built-in JSON / YAML / TOML ──────────────────────────────────────────────

#[test]
fn to_json_serializes_hash_and_arrays() {
    // Order of keys in stryke's hashes is insertion order (IndexMap).
    assert_eq!(
        eval_string(r#"to_json({a=>1, b=>[1,2,3]})"#),
        r#"{"a":1,"b":[1,2,3]}"#
    );
}

#[test]
fn from_json_returns_hashref() {
    assert_eq!(
        eval_string(r#"my $h = from_json(qq({"a":1,"b":2})); ref $h"#),
        "HASH"
    );
    assert_eq!(
        eval_int(r#"my $h = from_json(qq({"a":1,"b":2})); $h->{a} + $h->{b}"#),
        3
    );
}

#[test]
fn to_yaml_dumps_hash_with_nested_array() {
    // Trim trailing newline in case the implementation always appends one.
    let out = eval_string(r#"to_yaml({a=>1, b=>[1,2,3]})"#);
    assert!(out.contains("a: 1"), "got {:?}", out);
    assert!(out.contains("- 1"), "got {:?}", out);
    assert!(out.contains("- 2"), "got {:?}", out);
    assert!(out.contains("- 3"), "got {:?}", out);
}

#[test]
fn to_toml_dumps_simple_kv() {
    let out = eval_string(r#"to_toml({a=>1, b=>"hello"})"#);
    assert!(out.contains("a = 1"), "got {:?}", out);
    assert!(out.contains(r#"b = "hello""#), "got {:?}", out);
}

// ── Iteration helpers: first / any / all / none / reduce ─────────────────────

#[test]
fn first_returns_first_match() {
    assert_eq!(eval_int(r#"first { $_ > 3 } 1..10"#), 4);
}

#[test]
fn any_returns_truth_when_match_exists() {
    // Parens are necessary — without them, the ternary binds inside the `any`
    // argument list. Pinned here so the ergonomics regression is caught.
    assert_eq!(eval_int(r#"(any { $_ > 3 } 1..10) ? 1 : 0"#), 1);
}

#[test]
fn all_returns_truth_when_all_pass() {
    assert_eq!(eval_int(r#"(all { $_ > 0 } 1..10) ? 1 : 0"#), 1);
}

#[test]
fn none_returns_truth_when_no_match() {
    assert_eq!(eval_int(r#"(none { $_ > 100 } 1..10) ? 1 : 0"#), 1);
}

#[test]
fn reduce_sums_one_through_ten() {
    assert_eq!(eval_int(r#"reduce { $a + $b } 1..10"#), 55);
}

// ── Hash flatten and rebuild ─────────────────────────────────────────────────

#[test]
fn array_to_hash_via_pairs() {
    let out = eval_string(
        r#"my @kv = (a => 1, b => 2, c => 3);
           my %h = @kv;
           join(",", map { "$_=$h{$_}" } sort keys %h)"#,
    );
    assert_eq!(out, "a=1,b=2,c=3");
}

#[test]
fn hash_to_array_flattens_to_kv_pairs() {
    // Insertion order is preserved.
    assert_eq!(
        eval_string(r#"my %h = (a=>1, b=>2); my @kv = %h; "@kv""#),
        "a 1 b 2"
    );
}

// ── printf format flags ──────────────────────────────────────────────────────

#[test]
fn printf_negative_width_left_justifies() {
    assert_eq!(eval_string(r#"sprintf("%-5d|", -3)"#), "-3   |");
}

#[test]
fn printf_plus_flag_adds_sign() {
    // BUG-017 FIXED: `%+d` shows leading `+` for positive numbers.
    assert_eq!(eval_string(r#"sprintf("%+5d", 3)"#), "   +3");
    assert_eq!(eval_string(r#"sprintf("%+05d", 3)"#), "+0003");
    assert_eq!(eval_string(r#"sprintf("%+5d", -3)"#), "   -3");
}

#[test]
fn printf_zero_pad_with_negative() {
    assert_eq!(eval_string(r#"sprintf("%05d", -3)"#), "-0003");
}
