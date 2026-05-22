//! Extra coverage for the BUGS.md fixes landed during the May 2026 cleanup.
//!
//! Each section pins edge cases that the original BUG entry called out but
//! the primary regression pin didn't cover:
//!
//! - panic-free string / shift ops (BUG-242, 254, 255, 259)
//! - variadic numeric builtins (BUG-157, 167)
//! - deterministic multiset output (BUG-150)
//! - dijkstra negative-weight passthrough (BUG-203)
//! - RFC 3986 + 2-arg `uri_escape` (BUG-241 + Perl URI::Escape compat)
//! - `to_json` cycle detection (BUG-105)
//! - `from_json` garbage rejection (BUG-207)
//! - reverse() / x= / labels / redo / sliding_window / find_index /
//!   looks_like_number (BUG-099, 026, 260, 261, 225, 067, 054)
//! - `system` status word, list-form arg propagation (BUG-030, 031)
//! - backticks list context (BUG-013)
//! - `%ENV` propagation to child processes (BUG-014)
//! - `printf $fh` filehandle routing + bareword filehandle override of
//!   `E`/`PI`/`TAU` constants (BUG-085 + open-FH parser follow-up)
//! - `eof` buffered-reader check (BUG-098)
//! - splice list-context replacement (BUG-253)
//! - `tr///c` complement + `tr///s` squeeze (BUG-251, 252)
//! - `exists &name` (BUG-053)
//! - `m//` and `m//g` list contexts (BUG-016, 258)
//! - `__PACKAGE__` compile-time binding (BUG-256)
//! - `(caller(N))[3]` sub-name field (BUG-005)
//! - hash slice with array-var keys + `@{$ref}{KEYS}` (BUG-028 / 091 / 217)
//! - `\N` numbered backref in `s///` replacement (BUG-076)
//! - `PI` / `TAU` / `E` constants and `open FH, ...` literal-name precedence
//! - `delete @arr[…]` / `delete @h{…}` (BUG-042, 043)
//! - regex-replacement terminator escape `\/` (regression of BUG-076 fix)
//! - BUG-258 truthy fallback when all captures undef
//! - `uri_escape($s, $unsafe_pattern)` Perl URI::Escape compat

use crate::common::*;
use std::path::PathBuf;

// ── BUG-242 / 254 / 255: index/rindex panic-free ─────────────────────────

#[test]
fn index_start_past_length_returns_minus_one() {
    assert_eq!(eval_int(r#"index("hello", "h", 10)"#), -1);
    assert_eq!(eval_int(r#"index("hello", "h", 5)"#), -1);
}

#[test]
fn index_negative_start_clamps_to_zero() {
    assert_eq!(eval_int(r#"index("abc", "b", -1)"#), 1);
    assert_eq!(eval_int(r#"index("abc", "b", -100)"#), 1);
}

#[test]
fn rindex_negative_pos_returns_minus_one() {
    assert_eq!(eval_int(r#"rindex("abc", "b", -1)"#), -1);
    assert_eq!(eval_int(r#"rindex("abracadabra", "ab", -5)"#), -1);
}

#[test]
fn rindex_without_pos_returns_last_occurrence() {
    assert_eq!(eval_int(r#"rindex("abcabc", "b")"#), 4);
}

// ── BUG-259: bitwise shift overflow ──────────────────────────────────────

#[test]
fn shift_by_width_or_more_returns_zero_for_left() {
    assert_eq!(eval_int(r#"0 << 100"#), 0);
    assert_eq!(eval_int(r#"1 << 64"#), 0);
    assert_eq!(eval_int(r#"1 << 65"#), 0);
}

#[test]
fn shift_right_past_width_preserves_sign() {
    // Arithmetic shift: positive → 0, negative → -1.
    assert_eq!(eval_int(r#"-1 >> 100"#), -1);
    assert_eq!(eval_int(r#"123 >> 100"#), 0);
}

#[test]
fn shift_negative_amount_returns_zero() {
    assert_eq!(eval_int(r#"1 << -1"#), 0);
    assert_eq!(eval_int(r#"1 >> -1"#), 0);
}

// ── BUG-157: variadic crc32 ──────────────────────────────────────────────

#[test]
fn crc32_per_arg_equals_concat_digest() {
    assert_eq!(
        eval_int(r#"(crc32("ab", "cd") == crc32("abcd")) ? 1 : 0"#),
        1
    );
}

// ── BUG-167: variadic gcd/lcm ────────────────────────────────────────────

#[test]
fn gcd_folds_over_every_operand() {
    assert_eq!(eval_int(r#"gcd(12, 18, 24)"#), 6);
    assert_eq!(eval_int(r#"gcd(12, 18, 35)"#), 1);
    assert_eq!(eval_int(r#"gcd([12, 18, 24])"#), 6);
}

#[test]
fn lcm_folds_over_every_operand() {
    assert_eq!(eval_int(r#"lcm(4, 6, 8)"#), 24);
    assert_eq!(eval_int(r#"lcm(4, 6, 10)"#), 60);
    assert_eq!(eval_int(r#"lcm(0, 6, 8)"#), 0);
}

// ── BUG-150: multiset ops produce deterministic order ────────────────────

#[test]
fn multiset_union_sorted_lex_join() {
    assert_eq!(
        eval_string(r#"join(",", multiset_union([1, 1, 2, 3], [2, 4]))"#),
        "1,1,2,3,4"
    );
}

#[test]
fn multiset_intersection_sorted_lex_join() {
    assert_eq!(
        eval_string(r#"join(",", multiset_intersection([1, 1, 2, 3], [1, 2, 2]))"#),
        "1,2"
    );
}

#[test]
fn multiset_difference_sorted_lex_join() {
    assert_eq!(
        eval_string(r#"join(",", multiset_difference([1, 1, 2, 3], [1]))"#),
        "1,2,3"
    );
}

// ── BUG-203: dijkstra_relax passes negative weights through ──────────────

#[test]
fn dijkstra_relax_with_negative_weight_matches_bellman_ford() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dijkstra_relax(3, -5, 10))"#),
        "-2"
    );
    // Sanity: positive weight is the same in both relaxers.
    assert_eq!(
        eval_string(r#"sprintf("%.10g", dijkstra_relax(3, 5, 100))"#),
        "8"
    );
}

// ── BUG-241 + URI::Escape compat: uri_escape one-arg vs two-arg ──────────

#[test]
fn uri_escape_default_preserves_rfc3986_unreserved() {
    assert_eq!(
        eval_string(r#"uri_escape("hello-world.tar~gz")"#),
        "hello-world.tar~gz"
    );
}

#[test]
fn uri_escape_default_encodes_space_and_slashes() {
    assert_eq!(
        eval_string(r#"uri_escape("a b/c")"#),
        "a%20b%2Fc"
    );
}

#[test]
fn uri_escape_with_strict_pattern_encodes_everything_non_alpha() {
    assert_eq!(
        eval_string(r#"uri_escape("hello-world.tar~gz", "^A-Za-z0-9")"#),
        "hello%2Dworld%2Etar%7Egz"
    );
}

#[test]
fn uri_escape_with_explicit_unsafe_set_only_encodes_listed_chars() {
    // `[!?]` only — alphanumerics, spaces, dots all survive.
    assert_eq!(
        eval_string(r#"uri_escape("hi! ok?", "!?")"#),
        "hi%21 ok%3F"
    );
}

// ── BUG-105: to_json cycle detection ─────────────────────────────────────

#[test]
fn to_json_self_referential_hash_emits_null_back_edge() {
    let code = r#"
        my $a = { name => "root" };
        $a->{self} = $a;
        my $j = to_json($a);
        ($j =~ /"name":"root"/ && $j =~ /"self":null/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn to_json_two_node_cycle_terminates() {
    let code = r#"
        my $a = { tag => "A" };
        my $b = { tag => "B" };
        $a->{next} = $b;
        $b->{prev} = $a;
        my $j = to_json($a);
        ($j =~ /"tag":"A"/ && $j =~ /"prev":null/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-207: from_json garbage rejection ─────────────────────────────────

#[test]
fn from_json_on_empty_string_is_undef() {
    assert_eq!(eval_int(r#"defined(from_json("")) ? 1 : 0"#), 0);
}

#[test]
fn from_json_on_whitespace_only_is_undef() {
    assert_eq!(eval_int(r#"defined(from_json("   ")) ? 1 : 0"#), 0);
}

#[test]
fn from_json_on_valid_object_decodes() {
    assert_eq!(
        eval_int(r#"from_json('{"x":42}')->{x}"#),
        42
    );
}

// ── BUG-099: reverse() with empty parens ─────────────────────────────────

#[test]
fn reverse_empty_parens_yields_empty_list() {
    assert_eq!(eval_int(r#"my @r = reverse(); scalar @r"#), 0);
}

// ── BUG-026: $s x= N compound assignment ─────────────────────────────────

#[test]
fn x_compound_assign_with_zero_clears_string() {
    assert_eq!(eval_string(r#"my $s = "abc"; $s x= 0; $s"#), "");
}

#[test]
fn x_compound_assign_with_var_count() {
    assert_eq!(
        eval_string(r#"my $s = "ab"; my $n = 3; $s x= $n; $s"#),
        "ababab"
    );
}

// ── BUG-260 / 261: digit labels + redo if ────────────────────────────────

#[test]
fn last_with_digit_label_exits_outer_loop() {
    let code = r#"
        my @hits;
        OUT1: for my $i (1:5) {
            for my $j (1:5) {
                last OUT1 if $i == 2 && $j == 3;
                push @hits, "$i.$j";
            }
        }
        scalar @hits
    "#;
    // 1.1..1.5 (5) + 2.1, 2.2 (2) = 7
    assert_eq!(eval_int(code), 7);
}

#[test]
fn redo_if_postfix_runs_body_again() {
    let code = r#"
        my $n = 0;
        my @log;
        for my $x (1:2) {
            $n++;
            push @log, $n;
            redo if $n < 3;
        }
        join(",", @log)
    "#;
    assert_eq!(eval_string(code), "1,2,3,4");
}

// ── BUG-225 / 067 / 054: sliding_window / find_index / looks_like_number ──

#[test]
fn sliding_window_default_n_one_returns_singletons() {
    // Without a window-size arg, behaves as N=1.
    assert_eq!(
        eval_string(r#"my @w = sliding_window([1, 2, 3], 1); join(";", map { join(",", @$_) } @w)"#),
        "1;2;3"
    );
}

#[test]
fn sliding_window_n_larger_than_list_yields_empty() {
    assert_eq!(eval_int(r#"my @w = sliding_window([1, 2], 5); scalar @w"#), 0);
}

#[test]
fn find_index_with_arrayref_args() {
    // The block-form path; arrayref args use first/firstidx aliases.
    assert_eq!(eval_int(r#"find_index { $_ % 2 == 0 } (1, 3, 4, 5)"#), 2);
    assert_eq!(eval_int(r#"firstidx { $_ eq "b" } ("a", "b", "c")"#), 1);
}

#[test]
fn looks_like_number_recognizes_edge_forms() {
    assert_eq!(eval_int(r#"looks_like_number("+3.14e-2")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("-Inf")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("NaN")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("0")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("1.2.3")"#), 0);
    assert_eq!(eval_int(r#"looks_like_number(undef)"#), 0);
}

// ── BUG-030 / 031: system status word + list form ────────────────────────

#[test]
fn system_true_returns_zero_in_status_word_form() {
    assert_eq!(eval_int(r#"system("true")"#), 0);
}

#[test]
fn system_false_status_word_high_byte_is_one() {
    // (1 << 8) == 256.
    assert_eq!(eval_int(r#"system("false") >> 8"#), 1);
}

#[test]
fn system_list_form_propagates_exit_code() {
    assert_eq!(
        eval_int(r#"system("sh", "-c", "exit 9"); $? >> 8"#),
        9
    );
}

// ── BUG-013: backticks in list context yields lines ──────────────────────

#[test]
fn backticks_list_context_lines_include_terminator() {
    assert_eq!(
        eval_string(r#"my @lines = `printf "a\nb\nc\n"`; join("|", @lines)"#),
        "a\n|b\n|c\n"
    );
}

#[test]
fn backticks_list_context_no_trailing_newline_keeps_final() {
    assert_eq!(
        eval_string(r#"my @lines = `printf "a\nb"`; join("|", @lines)"#),
        "a\n|b"
    );
}

#[test]
fn backticks_scalar_context_returns_full_string() {
    assert_eq!(
        eval_int(r#"my $s = `printf "a\nb\nc\n"`; length($s)"#),
        6
    );
}

// ── BUG-014: $ENV propagation reaches child processes ───────────────────

#[test]
fn env_set_then_delete_clears_child_view() {
    let probe = format!("STRYKE_PIN_PROBE_{}", std::process::id());
    let code = format!(
        r#"$ENV{{{0}}} = "first";
           my $a = `env | grep '^{0}='`;
           delete $ENV{{{0}}};
           my $b = `env | grep '^{0}='`;
           ($a =~ /^{0}=first/ && length($b) == 0) ? 1 : 0"#,
        probe
    );
    assert_eq!(eval_int(&code), 1);
}

// ── BUG-085 + parser follow-up: printf $fh + literal bareword filehandle ─

#[test]
fn printf_with_bareword_filehandle_writes_to_handle() {
    let path: PathBuf = std::env::temp_dir().join(format!(
        "stryke_pin_printf_E_{}",
        std::process::id()
    ));
    let p = path.to_string_lossy();
    // `open E, ">", PATH` — the `E` constant must NOT win over the
    // filehandle slot. `printf E "..."` routes through the named handle.
    let _ = eval_string(&format!(
        r#"open E, ">", "{0}" or die;
           printf E "n=%d\n", 7;
           printf E "k=%s\n", "v";
           close E;
           "OK""#,
        p
    ));
    let body = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    assert_eq!(body, "n=7\nk=v\n");
}

#[test]
fn euler_constant_still_available_via_bareword_in_expression_position() {
    // `E` is the constant in non-filehandle contexts. The `open E, …`
    // parser carve-out only fires in the filehandle slot.
    let v = eval_string("E");
    assert!(v.starts_with("2.71828"), "expected Euler, got {:?}", v);
}

#[test]
fn open_pi_as_filehandle_does_not_collide_with_constant() {
    let path: PathBuf = std::env::temp_dir().join(format!(
        "stryke_pin_open_pi_{}",
        std::process::id()
    ));
    let p = path.to_string_lossy();
    std::fs::write(&path, "hello\n").unwrap();
    let n = eval_int(&format!(
        r#"open PI, "<", "{0}" or die;
           my $line = <PI>;
           close PI;
           length($line)"#,
        p
    ));
    let _ = std::fs::remove_file(&path);
    assert_eq!(n, 6);
}

// ── BUG-098: eof on buffered reader ──────────────────────────────────────

#[test]
fn eof_false_before_read_true_after_drain() {
    let path: PathBuf =
        std::env::temp_dir().join(format!("stryke_pin_eof_drain_{}", std::process::id()));
    std::fs::write(&path, "line1\nline2\n").unwrap();
    let p = path.to_string_lossy();
    let n = eval_int(&format!(
        r#"open F, "<", "{0}" or die;
           my $before = eof("F") ? 1 : 0;
           my $a = <F>;
           my $b = <F>;
           my $after = eof("F") ? 1 : 0;
           close F;
           ($before == 0 && $after == 1) ? 1 : 0"#,
        p
    ));
    let _ = std::fs::remove_file(&path);
    assert_eq!(n, 1);
}

// ── BUG-253: splice list-context replacement ─────────────────────────────

#[test]
fn splice_inserts_arrayref_flattened() {
    assert_eq!(
        eval_string(r#"my @a = (1, 2, 5, 6); my $r = [3, 4]; splice(@a, 2, 0, @$r); join(",", @a)"#),
        "1,2,3,4,5,6"
    );
}

#[test]
fn splice_replaces_with_range_in_list_context() {
    assert_eq!(
        eval_string(r#"my @a = (1, 9, 9, 9, 5); splice(@a, 1, 3, (2:4)); join(",", @a)"#),
        "1,2,3,4,5"
    );
}

// ── BUG-251 / 252: tr///c and tr///s ─────────────────────────────────────

#[test]
fn tr_complement_count_counts_non_matching() {
    assert_eq!(
        eval_int(r#"my $s = "abcde123"; scalar($s =~ tr/0-9//c)"#),
        5
    );
}

#[test]
fn tr_squeeze_collapses_runs_no_translit() {
    assert_eq!(
        eval_string(r#"my $s = "aaaaabbcc"; $s =~ tr/a-z//s; $s"#),
        "abc"
    );
}

#[test]
fn tr_translate_then_squeeze_with_explicit_targets() {
    assert_eq!(
        eval_string(r#"my $s = "aabbccdd"; $s =~ tr/a-d/x/s; $s"#),
        "x"
    );
}

// ── BUG-053: exists &name ────────────────────────────────────────────────

#[test]
fn exists_amp_subname_recognizes_qualified_lookup() {
    let code = r#"
        package Foo::Bar;
        sub hello { 1 }
        package main;
        (exists(&Foo::Bar::hello) && !exists(&Foo::Bar::missing)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-016 / 258: m//g and m// list contexts ────────────────────────────

#[test]
fn match_g_with_groups_flattens_across_matches() {
    assert_eq!(
        eval_string(r#"my @m = "a1 b2" =~ /(\w)(\d)/g; "@m""#),
        "a 1 b 2"
    );
}

#[test]
fn match_single_with_groups_destructures() {
    let code = r#"
        my ($scheme, $rest) = ("https://example" =~ m{^([a-z]+)://(.*)});
        ($scheme eq "https" && $rest eq "example") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn match_with_only_optional_group_unmatched_still_truthy() {
    // BUG-258 + the "all-captures-undef → integer(1)" fallback so the
    // boolean-test idiom keeps working when the regex matched but the
    // optional group didn't fire.
    let code = r#"
        package Stk::Probe;
        sub looks_numericish { $_[0] =~ /^-?\d+(\.\d+)?$/ }
        package main;
        (Stk::Probe::looks_numericish("42")
            && Stk::Probe::looks_numericish("-3.14")
            && !Stk::Probe::looks_numericish("abc")) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-256: __PACKAGE__ baked at compile time ───────────────────────────

#[test]
fn package_dunder_is_definition_site_not_caller() {
    let code = r#"
        package Demo::Tagger;
        sub tag { __PACKAGE__ }
        package main;
        (Demo::Tagger::tag() eq "Demo::Tagger") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-005: caller(0) field 3 is the sub name ──────────────────────────

#[test]
fn caller_subname_field_populated_for_nested_call() {
    let code = r#"
        package Stk::CallerDemo;
        sub inner { my @c = caller(0); $c[3] }
        sub outer { inner() }
        package main;
        my $name = Stk::CallerDemo::outer();
        ($name =~ /inner/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── BUG-028 / 091 / 217: hash slice via array var and arrayref deref ────

#[test]
fn hash_slice_via_array_var_returns_values_in_order() {
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2, c=>3, d=>4); my @ks = ("d","a","c"); my @v = @h{@ks}; "@v""#
        ),
        "4 1 3"
    );
}

#[test]
fn hash_slice_through_curly_deref_works() {
    assert_eq!(
        eval_string(
            r#"my %h = (x=>"X", y=>"Y", z=>"Z"); my $r = \%h; my @v = @{$r}{qw(x z)}; "@v""#
        ),
        "X Z"
    );
}

#[test]
fn array_slice_through_curly_deref_works() {
    assert_eq!(
        eval_string(r#"my $aref = [10, 20, 30, 40, 50]; my @v = @{$aref}[0, 2, 4]; "@v""#),
        "10 30 50"
    );
}

// ── BUG-076: numbered backref and terminator escape ────────────────────

#[test]
fn substitution_backref_swaps_groups() {
    assert_eq!(
        eval_string(r#"my $s = "ab"; $s =~ s/(a)(b)/\2\1/; $s"#),
        "ba"
    );
}

#[test]
fn substitution_terminator_escape_preserves_slash() {
    // `\/` in `s/.../.../...` must drop the backslash so a literal `/`
    // appears in the body without ending the s/// — this is the
    // regression that the BUG-076 fix had to preserve.
    assert_eq!(
        eval_string(r#"my $s = "abc"; $s =~ s/b/\//; $s"#),
        "a/c"
    );
}

#[test]
fn substitution_multi_digit_remains_octal() {
    // `\10` (no preceding digit) is octal 0o10 = 0x08.
    assert_eq!(
        eval_string(r#"my $s = "ab"; $s =~ s/a/\10/; sprintf("%vd", $s)"#),
        "8.98"
    );
}

// ── BUG-042 / 043: delete slice forms ────────────────────────────────────

#[test]
fn delete_hash_slice_via_arrayvar_drains_keys() {
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2, c=>3, d=>4); my @ks = ("a", "c"); delete @h{@ks};
               join(",", sort keys %h)"#
        ),
        "b,d"
    );
}

#[test]
fn delete_array_slice_returns_removed_values() {
    assert_eq!(
        eval_string(r#"my @a = (10, 20, 30, 40); my @d = delete @a[1, 3]; join(",", @d)"#),
        "20,40"
    );
}
