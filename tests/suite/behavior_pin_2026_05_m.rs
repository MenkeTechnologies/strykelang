//! Behavior-pinning batch M (2026-05-04): BEGIN/INIT/CHECK/END phasers,
//! class BUILD/BUILDARGS, struct/enum, regex backrefs, refaddr semantics,
//! sub attributes, postfix-for limits, integer wrap.

use crate::common::*;

// ── BEGIN / INIT / CHECK / END phaser order ─────────────────────────────────

#[test]
fn begin_block_mutations_to_package_vars_lost_today() {
    // BUG-078: BEGIN executes (you can see its prints when run via CLI), but
    // its writes to `our`-declared package variables do not persist into
    // the main body's compilation phase. The `$log` reset at the top wins.
    let out = eval_string(
        r#"our $log = "";
           BEGIN { $main::log .= "B1:" }
           $log .= "M1:";
           BEGIN { $main::log .= "B2:" }
           $log .= "M2:";
           $log"#,
    );
    assert_eq!(out, "M1:M2:");
}

#[test]
fn end_blocks_run_in_lifo_order_at_exit() {
    // We can't observe END from inside `eval` (it fires at interpreter exit,
    // not at lib eval). Just confirm the source parses.
    assert!(stryke::parse(r#"END { 1 } END { 2 } print "main""#).is_ok());
}

#[test]
fn init_block_parses() {
    assert!(stryke::parse(r#"INIT { 1 } print "main""#).is_ok());
}

#[test]
fn check_block_parses() {
    assert!(stryke::parse(r#"CHECK { 1 } print "main""#).is_ok());
}

// ── Class BUILD invoked at construction; BUILDARGS not invoked today ────────

#[test]
fn class_build_method_runs_at_construction() {
    let out = eval_string(
        r#"our $log = "";
           class Cat {
             name: Str = "?"
             fn BUILD { $main::log .= "BUILD:" . $self->name . ";" }
           }
           Cat(name => "Felix");
           $log"#,
    );
    assert_eq!(out, "BUILD:Felix;");
}

#[test]
fn class_buildargs_method_not_invoked_today() {
    // BUG-073: stryke does not call a `BUILDARGS` hook during construction.
    // BUILD is invoked normally; BUILDARGS would let users transform args
    // before storage but stryke skips it.
    let out = eval_string(
        r#"our $log = "";
           class Cat {
             name: Str = "?"
             fn BUILDARGS { $main::log .= "BUILDARGS:"; @_ }
             fn BUILD     { $main::log .= "BUILD:" }
           }
           Cat(name => "Felix");
           $log"#,
    );
    assert_eq!(out, "BUILD:");
}

#[test]
fn class_self_dot_new_can_be_overridden() {
    assert_eq!(
        eval_string(
            r#"class Cat {
                 name: Str = "?"
                 fn Self.new($cls, %args) { Cat(name => $args{name} . "!") }
               }
               Cat::new("Cat", name => "Felix")->name"#
        ),
        "Felix!"
    );
}

// ── struct ───────────────────────────────────────────────────────────────────

#[test]
fn struct_positional_construction_assigns_fields() {
    assert_eq!(
        eval_string(r#"struct Pt { x => Int, y => Int } my $p = Pt(3, 4); "$p->{x}/$p->{y}""#),
        "3/4"
    );
}

#[test]
fn struct_does_not_have_pkg_new_today() {
    // BUG-074: structs lack a `Pkg::new(...)` constructor. Use the bareword
    // form `Pt(3, 4)`.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"struct Pt { x => Int, y => Int } Pt::new(3, 4)"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::UndefinedSubroutine),
        "expected undefined-subroutine, got {:?}",
        kind
    );
}

// ── enum ────────────────────────────────────────────────────────────────────

#[test]
fn enum_member_stringifies_to_qualified_name() {
    assert_eq!(
        eval_string(r#"enum Color { Red, Green, Blue } "" . Color::Red"#),
        "Color::Red"
    );
}

#[test]
fn enum_members_compare_equal_to_self() {
    assert_eq!(
        eval_int(
            r#"enum Color { Red, Green, Blue }
               my $c = Color::Red;
               ($c == Color::Red) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn enum_members_compare_unequal_to_other_member() {
    assert_eq!(
        eval_int(
            r#"enum Color { Red, Green, Blue }
               my $c = Color::Red;
               ($c == Color::Green) ? 1 : 0"#
        ),
        0
    );
}

// ── Field validation ────────────────────────────────────────────────────────

#[test]
fn class_field_setter_rejects_wrong_type() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"class P { x: Int = 0 } my $p = P(); $p->x("string")"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected type error, got {:?}",
        kind
    );
}

#[test]
fn class_field_setter_accepts_correct_type() {
    assert_eq!(
        eval_int(r#"class P { x: Int = 0 } my $p = P(); $p->x(10); $p->x"#),
        10
    );
}

#[test]
fn class_private_field_rejects_external_access() {
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"class Frozen { priv v: Int = 0 } my $f = Frozen(); $f->v"#);
    assert!(
        matches!(
            kind,
            ErrorKind::Runtime | ErrorKind::Type | ErrorKind::Syntax
        ),
        "expected access error, got {:?}",
        kind
    );
}

// ── refaddr returns different addresses for `\@a` taken twice ───────────────

#[test]
fn refaddr_of_repeated_backslash_at_returns_different_addresses_today() {
    // BUG-075: in Perl, multiple `\@a` references all share the array's
    // address. In stryke, each `\@a` evaluates to a fresh ref-cell, so
    // refaddr returns different values.
    assert_eq!(
        eval_int(
            r#"my @a; my $r1 = \@a; my $r2 = \@a;
               refaddr($r1) == refaddr($r2) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn refaddr_of_aliased_scalar_is_same() {
    // Aliasing a single ref via `=` keeps the same refaddr.
    assert_eq!(
        eval_int(
            r#"my @a; my $r = \@a; my $s = $r;
               refaddr($r) == refaddr($s) ? 1 : 0"#
        ),
        1
    );
}

// ── \1 vs $1 in s/// replacement ────────────────────────────────────────────

#[test]
fn backslash_one_in_substitution_inserts_capture() {
    // `\1` (single-digit, no trailing octal digit) is a numbered backref in
    // s/// replacement strings, matching Perl. Multi-digit forms like `\010`
    // still resolve as octal escapes.
    assert_eq!(
        eval_string(r#"my $s = "ab123cd"; $s =~ s/(\d+)/[\1]/; $s"#),
        "ab[123]cd"
    );
}

#[test]
fn dollar_one_in_substitution_inserts_capture() {
    assert_eq!(
        eval_string(r#"my $s = "ab123cd"; $s =~ s/(\d+)/[$1]/; $s"#),
        "ab[123]cd"
    );
}

// ── Regex named backref ─────────────────────────────────────────────────────

#[test]
fn named_backref_via_g_brace() {
    assert_eq!(eval_int(r#""abcabc" =~ /^(?<w>\w+)\g{w}$/ ? 1 : 0"#), 1);
}

#[test]
fn named_backref_via_k_angle() {
    assert_eq!(eval_int(r#""abcabc" =~ /^(?<w>\w+)\k<w>$/ ? 1 : 0"#), 1);
}

#[test]
fn numeric_backref_g_brace() {
    assert_eq!(eval_int(r#""abcabc" =~ /^(\w+)\g{1}$/ ? 1 : 0"#), 1);
}

// ── Regex (?(DEFINE)...) named subroutine reference ─────────────────────────

#[test]
fn regex_named_subroutine_pattern_compiles() {
    // PCRE2 supports named subroutine patterns; just confirm it matches.
    assert_eq!(
        eval_int(
            r#""abc123" =~ /
                 (?(DEFINE) (?<word>\w+) )
                 (?&word)
               /x ? 1 : 0"#
        ),
        1
    );
}

// ── Multiline qq / heredoc with code interpolation ─────────────────────────

#[test]
fn multiline_qq_includes_newlines_in_length() {
    // qq{...} body: "\nhello\ngoodbye\n" = 1 + 5 + 1 + 7 + 1 = 15 bytes.
    assert_eq!(
        eval_int(
            r#"my $s = qq{
hello
goodbye
}; length($s)"#
        ),
        15
    );
}

#[test]
fn heredoc_code_interp_via_dollar_brace_backslash() {
    let out = eval_string("my $x = 10; my $s = <<\"EOF\";\nv=$x d=${\\ ($x*2)}\nEOF\n$s");
    assert_eq!(out, "v=10 d=20\n");
}

// ── Multiline regex /m and /g ───────────────────────────────────────────────

#[test]
fn multiline_regex_m_g_collects_per_line_matches() {
    assert_eq!(
        eval_string(
            r#"my $s = "alpha\nbeta\ngamma";
               my @lines = ($s =~ /^(\w+)$/mg);
               "@lines""#
        ),
        "alpha beta gamma"
    );
}

// ── Sub attribute `:pure` ────────────────────────────────────────────────────

#[test]
fn fn_with_attribute_pure_runs_normally() {
    // Stryke parses `:pure` as an attribute hint and the function still runs.
    assert_eq!(eval_int(r#"fn add($a, $b) :pure { $a + $b } add(2, 3)"#), 5);
}

// ── Reverse a string in scalar context ─────────────────────────────────────

#[test]
fn scalar_reverse_reverses_string() {
    assert_eq!(eval_string(r#"scalar(reverse("abc"))"#), "cba");
}

// ── Postfix `for` modifier on `my @r = ...` form is rejected today ─────────

#[test]
fn postfix_for_on_my_at_assign_is_rejected_today() {
    // BUG-077: `my @r = myff(...) for (-1, 1)` raises "postfix `for` is not
    // supported on this statement form". Workaround: write the loop
    // explicitly.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"sub myff { @_ } my @r = myff($_ > 0 ? "p" : "n") for (-1, 1)"#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

#[test]
fn postfix_for_on_simple_expression_works() {
    assert_eq!(eval_string(r#"my $r = ""; $r .= "x" for 1..3; $r"#), "xxx");
}

// ── Integer wrap / overflow ────────────────────────────────────────────────

#[test]
fn i64_max_plus_one_wraps_to_min() {
    // 9_223_372_036_854_775_807 + 1 wraps in i64.
    assert_eq!(eval_int("9223372036854775807 + 1"), i64::MIN);
}

#[test]
fn pow_2_62_plus_pow_2_62_falls_back_to_float() {
    // 2^62 + 2^62 = 2^63 overflows i64; stryke uses float repr.
    let s = eval_string("2**62 + 2**62");
    assert!(
        s.contains("e+18") || s.contains("e18"),
        "expected scientific notation, got {:?}",
        s
    );
}

// ── String comparators distinguish from numeric ─────────────────────────────

#[test]
fn string_gt_lex_orders_10_before_9() {
    // "10" gt "9" is FALSE because '1' < '9' lexically.
    assert_eq!(eval_int(r#""10" gt "9" ? 1 : 0"#), 0);
}

#[test]
fn numeric_gt_orders_10_after_9() {
    assert_eq!(eval_int(r#""10" > "9" ? 1 : 0"#), 1);
}

// ── Pre vs post increment semantics ─────────────────────────────────────────

#[test]
fn post_increment_returns_old_value() {
    assert_eq!(
        eval_string(r#"my $a = 5; my $b = $a++; "a=$a b=$b""#),
        "a=6 b=5"
    );
}

#[test]
fn pre_increment_returns_new_value() {
    assert_eq!(
        eval_string(r#"my $a = 5; my $b = ++$a; "a=$a b=$b""#),
        "a=6 b=6"
    );
}

// ── Shared hashref through multiple variables ──────────────────────────────

#[test]
fn shared_hashref_writes_visible_through_all_aliases() {
    assert_eq!(
        eval_string(
            r#"my $h = {a=>1};
               my $r = $h;
               $r->{b} = 2;
               join(",", sort keys %$h)"#
        ),
        "a,b"
    );
}

// ── Chained assignment ──────────────────────────────────────────────────────

#[test]
fn chained_assignment_propagates_value() {
    assert_eq!(
        eval_string(r#"my ($a, $b, $c); $a = $b = $c = 5; "a=$a b=$b c=$c""#),
        "a=5 b=5 c=5"
    );
}

// ── reduce on empty list returns undef ─────────────────────────────────────

#[test]
fn reduce_empty_list_returns_undef() {
    assert_eq!(
        eval_int(r#"my $r = reduce { $a + $b } (); defined($r) ? 1 : 0"#),
        0
    );
}

#[test]
fn reduce_single_element_returns_element() {
    assert_eq!(eval_int(r#"reduce { $a + $b } (5)"#), 5);
}

// ── Map block returning multi-value list expands @r ─────────────────────────

#[test]
fn map_block_returns_pair_per_iteration_doubling_array_size() {
    assert_eq!(
        eval_string(r#"my @r = map { ($_, "x" x $_) } 1..3; "@r""#),
        "1 x 2 xx 3 xxx"
    );
}

// ── Splice with negative offset ────────────────────────────────────────────

#[test]
fn splice_with_negative_offset_and_count() {
    assert_eq!(
        eval_string(r#"my @a = (1..10); splice(@a, -3, 2); "@a""#),
        "1 2 3 4 5 6 7 10"
    );
}

// ── Sort of refs by a field ─────────────────────────────────────────────────

#[test]
fn sort_of_hashrefs_by_field() {
    assert_eq!(
        eval_string(
            r#"my @items = ({n=>"b"}, {n=>"a"}, {n=>"c"});
               my @s = sort { $a->{n} cmp $b->{n} } @items;
               join(",", map { $_->{n} } @s)"#
        ),
        "a,b,c"
    );
}

// ── Return from inside loop ─────────────────────────────────────────────────

#[test]
fn return_from_inside_for_loop_short_circuits_caller() {
    assert_eq!(
        eval_int(
            r#"sub mysearch { for my $i (1..5) { return $i if $i == 3 } -1 }
               mysearch()"#
        ),
        3
    );
}

#[test]
fn last_with_label_breaks_outer_loop_and_records_value() {
    assert_eq!(
        eval_int(
            r#"my $found;
               LOOP: for my $i (1..10) { if ($i == 4) { $found = $i; last LOOP } }
               $found"#
        ),
        4
    );
}

// ── Nested eval $@ propagation rule ─────────────────────────────────────────

#[test]
fn nested_eval_die_rethrow_preserves_message() {
    let out = eval_string(
        r#"my $log = "";
           eval {
             eval { die "first\n"; };
             $log .= "in:" . $@;
             die $@;
           };
           $log .= "out:" . $@;
           $log"#,
    );
    assert_eq!(out, "in:first\nout:first\n");
}

// ── BEGIN with use strict in same compilation unit ─────────────────────────

#[test]
fn use_strict_with_my_declaration_compiles() {
    assert!(
        stryke::parse("use strict; my $x = 5; print $x").is_ok(),
        "use strict + my should parse"
    );
    assert_eq!(eval_int("use strict; my $x = 5; $x"), 5);
}

// ── Heredoc inside a sub returns body ──────────────────────────────────────

#[test]
fn heredoc_inside_sub_returns_body() {
    assert_eq!(
        eval_string(
            "sub greet {
              return <<\"END\";
hello world
END
            }
            greet()"
        ),
        "hello world\n"
    );
}

// ── File test on directory and missing path ────────────────────────────────

#[test]
fn slash_tmp_is_writable_directory() {
    assert_eq!(eval_int(r#"-d "/tmp" && -w "/tmp" ? 1 : 0"#), 1);
}

#[test]
fn missing_path_is_neither_file_nor_dir() {
    assert_eq!(
        eval_int(r#"-f "/no/such/path/xx" || -d "/no/such/path/xx" ? 1 : 0"#),
        0
    );
}
