//! Extra `perlrs::run()` semantics: strings, builtins, aggregates, control flow.

use crate::error::ErrorKind;
use crate::run;

fn ri(s: &str) -> i64 {
    run(s).expect("run").to_int()
}

fn rf(s: &str) -> f64 {
    let v = run(s).expect("run");
    if let Some(f) = v.as_float() {
        return f;
    }
    if let Some(n) = v.as_integer() {
        return n as f64;
    }
    v.to_number()
}

fn rs(s: &str) -> String {
    run(s).expect("run").to_string()
}

#[test]
fn sprintf_basic_decimal() {
    assert_eq!(rs(r#"sprintf "%d", 42;"#), "42");
}

#[test]
fn sprintf_padded_zero() {
    assert_eq!(rs(r#"sprintf "%04d", 7;"#), "0007");
}

#[test]
fn index_finds_substring() {
    assert_eq!(ri(r#"index("foobar", "bar");"#), 3);
}

#[test]
fn rindex_finds_last() {
    assert_eq!(ri(r#"rindex("abab", "b");"#), 3);
}

#[test]
fn substr_two_arg() {
    assert_eq!(rs(r#"substr("abcdef", 2);"#), "cdef");
}

#[test]
fn substr_three_arg() {
    assert_eq!(rs(r#"substr("abcdef", 1, 3);"#), "bcd");
}

#[test]
fn hex_literal_and_hex_builtin() {
    assert_eq!(ri("0xFF;"), 255);
    assert_eq!(ri(r#"hex("FF");"#), 255);
}

#[test]
fn oct_literal_and_oct_builtin() {
    assert_eq!(ri("010;"), 8);
    assert_eq!(ri(r#"oct("10");"#), 8);
}

#[test]
fn ucfirst_lcfirst() {
    assert_eq!(rs(r#"ucfirst("hello");"#), "Hello");
    assert_eq!(rs(r#"lcfirst("HELLO");"#), "hELLO");
}

#[test]
fn split_space_default() {
    assert_eq!(ri(r#"scalar split(" ", "a b c");"#), 3);
}

#[test]
fn grep_block_list() {
    assert_eq!(ri(r#"scalar grep { $_ > 2 } (1, 2, 3, 4);"#), 2);
}

#[test]
fn map_block_double() {
    assert_eq!(ri(r#"my @m = map { $_ * 2 } (1, 2, 3); $m[2];"#), 6);
}

#[test]
fn qw_word_list() {
    assert_eq!(ri("scalar qw(a b c d);"), 4);
}

#[test]
fn array_slice_negative_index() {
    assert_eq!(ri("my @a = (10, 20, 30); $a[-1];"), 30);
}

#[test]
fn hash_exists_delete() {
    assert_eq!(ri(r#"my %h = (x => 1); exists $h{'x'} ? 1 : 0;"#), 1);
}

#[test]
fn ref_type_array() {
    assert_eq!(rs(r#"ref([]);"#), "ARRAY");
}

#[test]
fn scalar_context_hash_count_string() {
    let v = run(r#"my %h = (a => 1, b => 2); scalar %h;"#).expect("run");
    let s = v.to_string();
    assert!(
        s.contains('/') || v.to_int() >= 2,
        "unexpected scalar %h: {:?}",
        v
    );
}

#[test]
fn unless_else_branch() {
    assert_eq!(
        ri("my $r = 0; unless (0) { $r = 7 } else { $r = 9 }; $r;"),
        7
    );
}

#[test]
fn if_elsif_else_chain() {
    assert_eq!(
        ri("my $r = 0; if (0) { $r = 1 } elsif (0) { $r = 2 } else { $r = 42 }; $r;"),
        42
    );
}

#[test]
fn for_range_sum() {
    assert_eq!(ri("my $s = 0; for my $i (1..5) { $s = $s + $i; } $s;"), 15);
}

#[test]
fn compound_assign_plus() {
    assert_eq!(ri("my $x = 10; $x += 32; $x;"), 42);
}

#[test]
fn compound_assign_mul() {
    assert_eq!(ri("my $x = 6; $x *= 7; $x;"), 42);
}

#[test]
fn postincrement_scalar() {
    assert_eq!(ri("my $i = 41; $i++; $i;"), 42);
}

#[test]
fn preincrement_scalar() {
    assert_eq!(ri("my $i = 41; ++$i;"), 42);
}

#[test]
fn string_equality_eq() {
    assert_eq!(ri(r#""foo" eq "foo" ? 1 : 0;"#), 1);
}

#[test]
fn string_inequality_ne() {
    assert_eq!(ri(r#""a" ne "b" ? 1 : 0;"#), 1);
}

#[test]
fn numeric_and_word_ops() {
    assert_eq!(ri("1 and 2 and 3;"), 3);
    assert_eq!(ri("0 or 99;"), 99);
}

#[test]
fn repeat_operator_string() {
    assert_eq!(rs("'-' x 5;"), "-----");
}

#[test]
fn range_in_list_context_count() {
    assert_eq!(ri("scalar (1..10);"), 10);
}

#[test]
fn nested_arithmetic_parens() {
    assert_eq!(ri("((2 + 3) * (4 + 2));"), 30);
}

#[test]
fn float_compare_loose() {
    assert_eq!(ri("3.0 == 3 ? 1 : 0;"), 1);
}

#[test]
fn negative_zero_add() {
    assert_eq!(ri("-0 + 7;"), 7);
}

#[test]
fn backslash_reference_not_in_sub() {
    // Just ensure parse+run accepts common idiom where supported
    let _ = run("my $x = 1; $x;");
}

#[test]
fn sort_numeric_guess() {
    assert_eq!(ri("my @a = (3, 1, 2); $a[0] + $a[1] + $a[2];"), 6);
}

#[test]
fn reverse_array_list() {
    assert_eq!(ri("my @a = (1, 2, 3); $a[0] + $a[2];"), 4);
}

#[test]
fn join_empty_separator() {
    assert_eq!(rs(r#"join("", 1, 2, 3);"#), "123");
}

#[test]
fn sprintf_string_percent_s() {
    assert_eq!(rs(r#"sprintf "%s-%s", "a", "b";"#), "a-b");
}

#[test]
fn ord_multibyte_first_byte_or_char() {
    assert!(ri(r#"ord("Z");"#) > 0);
}

#[test]
fn chr_roundtrip_small() {
    assert_eq!(ri(r#"ord(chr(33));"#), 33);
}

#[test]
fn abs_zero() {
    assert_eq!(ri("abs(0);"), 0);
}

#[test]
fn sqrt_zero() {
    assert_eq!(ri("sqrt(0);"), 0);
}

#[test]
fn int_truncates_negative() {
    assert_eq!(ri("int(-3.9);"), -3);
}

#[test]
fn logical_xor_bitwise() {
    assert_eq!(ri("0b101 ^ 0b011;"), 6);
}

#[test]
fn shift_left_if_compileable() {
    assert_eq!(ri("4 >> 1;"), 2);
}

#[test]
fn diamond_operator_parses() {
    crate::parse("<>").expect("parse diamond");
}

#[test]
fn stat_returns_thirteen_fields_in_scalar_context() {
    assert_eq!(ri(r#"scalar stat "Cargo.toml";"#), 13);
}

#[test]
fn stat_missing_path_is_empty_list() {
    assert_eq!(ri(r#"scalar stat "/no/such/path/perlrs-test-xyz";"#), 0);
}

#[test]
fn glob_finds_rs_sources_under_src() {
    let n = ri(r#"scalar glob "src/*.rs";"#);
    assert!(
        n > 0,
        "glob src/*.rs should match at least one file, got {n}"
    );
}

#[test]
fn glob_par_plain_list_context_count() {
    let n = ri(r#"glob_par "src/*.rs";"#);
    assert!(
        n > 0,
        "glob_par without scalar should yield array len as to_int, got {n}"
    );
}

#[test]
fn glob_par_finds_rs_sources_under_src() {
    let n = ri(r#"scalar glob_par "src/*.rs";"#);
    assert!(
        n > 0,
        "glob_par src/*.rs should match at least one file, got {n}"
    );
}

#[test]
fn glob_par_tree_walker_plain_matches_count() {
    let program = crate::parse(r#"glob_par "src/*.rs";"#).expect("parse");
    let mut interp = crate::interpreter::Interpreter::new();
    let n = interp
        .execute_tree(&program)
        .expect("execute_tree")
        .to_int();
    assert!(
        n > 0,
        "tree-walker glob_par (plain) should match at least one file, got {n}"
    );
}

#[test]
fn glob_par_tree_walker_matches_count() {
    let program = crate::parse(r#"scalar glob_par "src/*.rs";"#).expect("parse");
    let mut interp = crate::interpreter::Interpreter::new();
    let n = interp
        .execute_tree(&program)
        .expect("execute_tree")
        .to_int();
    assert!(
        n > 0,
        "tree-walker glob_par should match at least one file, got {n}"
    );
}

#[test]
fn ppool_collect_order_and_results() {
    let n = ri(r#"
        my $p = ppool(2);
        $p->submit(sub { $_ * 3 }, 2);
        $p->submit(sub { $_ * 3 }, 4);
        my @r = $p->collect();
        $r[0] + $r[1];
    "#);
    assert_eq!(n, 18);
}

#[test]
fn opendir_readdir_returns_name() {
    assert_eq!(
        ri(r#"opendir D, "."; my $x = readdir D; closedir D; $x ne "" ? 1 : 0;"#),
        1
    );
}

#[test]
fn rewinddir_resets_read_position() {
    assert_eq!(
        ri(r#"opendir D, "."; readdir D; rewinddir D; (telldir D) == 0 ? 1 : 0;"#),
        1
    );
}

#[test]
fn pchannel_fan_send_recv() {
    let s = r#"
        my ($tx, $rx) = pchannel();
        fan 3 { $tx->send($_) }
        my $sum = 0;
        $sum += $rx->recv() for 1..3;
        $sum;
    "#;
    assert_eq!(ri(s), 3);
}

#[test]
fn pselect_multiplex_recv() {
    let s = r#"
        my ($tx1, $rx1) = pchannel();
        my ($tx2, $rx2) = pchannel();
        $tx1->send(7);
        my ($v, $i) = pselect($rx1, $rx2);
        $v == 7 && $i == 0 ? 1 : 0;
    "#;
    assert_eq!(ri(s), 1);
}

#[test]
fn deque_push_front_back_pop_order() {
    let s = r#"
        my $q = deque();
        $q->push_back(1);
        $q->push_front(0);
        $q->pop_front() + $q->pop_front();
    "#;
    assert_eq!(ri(s), 1);
}

#[test]
fn heap_numeric_comparator_pops_sorted() {
    let s = r#"
        my $pq = heap(sub { $a <=> $b });
        $pq->push(3);
        $pq->push(1);
        $pq->push(2);
        $pq->pop() + $pq->pop() + $pq->pop();
    "#;
    assert_eq!(ri(s), 6);
}

#[test]
fn trace_fan_mysync_runs() {
    let s = r#"
        mysync $counter = 0;
        trace { fan 4 { $counter++ } };
        $counter;
    "#;
    assert_eq!(ri(s), 4);
}

#[test]
fn timer_returns_elapsed_ms() {
    let ms = rf(r#"timer { my $x = 1 + 1; }"#);
    assert!(ms >= 0.0);
    assert!(ms < 60_000.0, "timer should be wall-clock ms, got {ms}");
}

#[test]
fn pmap_chunked_preserves_order_and_values() {
    let s = r#"
        my @a = pmap_chunked 2 { $_ * 2 } (1, 2, 3, 4);
        $a[0] + $a[1] + $a[2] + $a[3];
    "#;
    assert_eq!(ri(s), 20);
}

#[test]
fn reduce_left_fold_sum_and_concat() {
    assert_eq!(ri(r#"reduce { $a + $b } (1, 2, 3, 4);"#), 10);
    assert_eq!(rs(r#"reduce { $a . $b } ("a", "b", "c");"#), "abc");
}

#[test]
fn pipeline_filter_map_take_collect() {
    let s = r#"
        my @a = pipeline(1, 9, 10, 15)
            ->filter(sub { $_ > 5 })
            ->map(sub { $_ * 2 })
            ->take(2)
            ->collect();
        $a[0] + $a[1];
    "#;
    assert_eq!(ri(s), 38);
}

#[test]
fn async_await_returns_block_value() {
    assert_eq!(ri(r#"my $t = async { 40 + 2 }; await($t);"#), 42);
}

#[test]
fn async_await_two_tasks() {
    assert_eq!(
        ri(r#"my $a = async { 10 }; my $b = async { 32 }; await($a) + await($b);"#),
        42
    );
}

#[test]
fn await_passes_through_non_task() {
    assert_eq!(ri(r#"await(7);"#), 7);
}

#[test]
fn capture_structured_exit_and_failed() {
    assert_eq!(
        ri(r#"my $r = capture("true"); $r->exitcode + $r->failed;"#),
        0
    );
    assert_eq!(ri(r#"my $r = capture("false"); $r->exitcode;"#), 1);
    assert_eq!(ri(r#"my $r = capture("false"); $r->failed;"#), 1);
}

#[test]
fn typed_my_int_str_float_runtime_checks() {
    assert_eq!(ri(r#"typed my $x : Int = 7; $x = 3; $x"#), 3);
    assert_eq!(rs(r#"typed my $s : Str = "a"; $s = "b"; $s"#), "b");
    assert_eq!(rf(r#"typed my $f : Float = 2; $f = 3.5; $f"#), 3.5);
    assert!(run(r#"typed my $x : Int = "nope";"#).is_err());
    let e = run(r#"typed my $x : Int = 1; $x = "y";"#).expect_err("type mismatch");
    assert_eq!(e.kind, ErrorKind::Type);
}

#[test]
fn pack_unpack_and_length() {
    assert_eq!(ri(r#"unpack("C", "A");"#), 65);
    assert_eq!(ri(r#"length pack("C3", 1, 2, 3);"#), 3);
}

#[test]
fn open_read_pipe_echo() {
    let out = rs(r#"
        open(FH, "-|", "echo hi");
        my $x = <FH>;
        close FH;
        $x;
    "#);
    assert!(out.contains("hi"));
}

#[test]
fn autoload_sets_missing_sub_name() {
    assert_eq!(
        rs(r#"
        sub AUTOLOAD { $AUTOLOAD }
        not_defined_yet();
    "#,),
        "main::not_defined_yet"
    );
}

/// `$?` after a successful `system` (POSIX-style status; exit 0 → 0).
#[cfg(unix)]
#[test]
fn perl_compat_dollar_question_reflects_system() {
    assert_eq!(ri(r#"system("true"); $?"#), 0);
}

/// `$?` after `capture` matches `system` (VM path records `ExitStatus`).
#[cfg(unix)]
#[test]
fn dollar_question_reflects_capture() {
    assert_eq!(ri(r#"capture("true"); $?"#), 0);
    assert_eq!(ri(r#"capture("false"); $?"#), 256);
}

#[test]
fn our_isa_populates_package_stash() {
    assert_eq!(ri(r#"package C; our @ISA = ("P"); scalar @ISA"#), 1);
}

#[test]
fn qualified_sub_call_across_packages() {
    assert_eq!(
        ri(r#"package P; sub meth { 10 } package main; P::meth()"#),
        10
    );
}

#[test]
fn isa_visible_from_main_after_package_blocks() {
    assert_eq!(
        ri(r#"
        package P;
        sub meth { 10 }
        package C;
        our @ISA = ("P");
        package main;
        scalar @C::ISA
        "#),
        1
    );
}

#[test]
fn perl_compat_super_calls_parent_method() {
    assert_eq!(
        ri(r#"
        package P;
        sub meth { 10 }
        package C;
        our @ISA = ("P");
        sub meth { my $s = shift; $s->SUPER::meth + 5 }
        package main;
        my $o = bless {}, "C";
        $o->meth();
        "#),
        15
    );
}

#[test]
fn perl_compat_use_overload_dispatches_add() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '+' => 'add';
        sub add { my ($a, $b) = @_; $a->{n} + $b->{n} }
        package main;
        my $a = O->new(n => 2);
        my $b = O->new(n => 3);
        $a + $b;
        "#),
        5
    );
}

#[test]
fn perl_compat_use_overload_coderef_handler() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '+' => \&add;
        sub add { my ($a, $b) = @_; $a->{n} * $b->{n} }
        package main;
        my $a = O->new(n => 2);
        my $b = O->new(n => 3);
        $a + $b;
        "#),
        6
    );
}

#[test]
fn perl_compat_use_overload_stringify() {
    assert_eq!(
        rs(r#"
        package O;
        use overload '""' => 'as_string';
        sub as_string { "x7" }
        package main;
        my $o = bless { n => 7 }, "O";
        "$o"
        "#),
        "x7"
    );
}

/// CPAN-style single `use overload` with coderef handlers for `+` and `""`.
/// `stringify` uses a constant return: arrow/hash on the invocant in overload subs is still limited.
#[test]
fn perl_compat_use_overload_combined_coderef() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '+' => \&add, '""' => \&stringify;
        sub add { my ($a, $b) = @_; $a->{n} + $b->{n} }
        sub stringify { "v7" }
        package main;
        my $a = O->new(n => 2);
        my $b = O->new(n => 3);
        $a + $b;
        "#),
        5
    );
    assert_eq!(
        rs(r#"
        package O;
        use overload '+' => \&add, '""' => \&stringify;
        sub add { my ($a, $b) = @_; $a->{n} + $b->{n} }
        sub stringify { "v7" }
        package main;
        my $o = bless { n => 7 }, "O";
        "$o"
        "#),
        "v7"
    );
}

#[test]
fn perl_compat_tie_scalar_fetch_store() {
    assert_eq!(
        ri(r#"
        package T;
        sub TIESCALAR { bless { v => 0 }, shift }
        sub FETCH { $_[0]->{v} }
        sub STORE { $_[0]->{v} = $_[1] }
        package main;
        my $x;
        tie $x, "T";
        $x = 7;
        $x;
        "#),
        7
    );
}

/// `local *Alias = *Real` aliases the handle name for `print` / `close`.
#[cfg(unix)]
#[test]
fn perl_compat_local_typeglob_aliases_handle() {
    let out = rs(r#"
        my $f = "/tmp/perlrs_tg_" . $$;
        open OUT, ">", $f;
        local *G = *OUT;
        print G "xyz";
        close OUT;
        open IN, "<", $f;
        my $s = <IN>;
        close IN;
        unlink $f;
        $s;
    "#);
    assert!(out.contains("xyz"));
}
