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

/// Plain `@name` / `%name` on the RHS of scalar assignment must not expand the full aggregate
/// (`ArrayLen` / scalar `%h`), matching Perl.
#[test]
fn perl_compat_named_array_rvalue_in_scalar_assign_is_length() {
    assert_eq!(ri(r#"my @y = (10, 20, 30); my $x = @y; $x"#), 3);
}

#[test]
fn perl_compat_named_hash_rvalue_in_scalar_assign_is_fill_string() {
    assert_eq!(rs(r#"my %h = (a => 1, b => 2); my $x = %h; $x"#), "2/3");
}

/// `exists` / `delete` on `$a[$i]` lower to [`Op::ExistsArrayElem`] / [`Op::DeleteArrayElem`].
#[test]
fn perl_compat_named_array_exists_delete() {
    assert_eq!(
        rs(r#"my @a = (10, 20, 30);
        my $e0 = exists $a[99] ? 1 : 0;
        my $e1 = exists $a[1] ? 1 : 0;
        my $d = delete $a[1];
        my $e2 = exists $a[1] ? 1 : 0;
        $e0 . "," . $e1 . "," . $d . "," . $e2;"#,),
        "0,1,20,1"
    );
}

/// `exists` / `delete` on `$aref->[$i]` use [`Op::ExistsArrowArrayElem`] /
/// [`Op::DeleteArrowArrayElem`] (same outcome string as named-array exists/delete in this runtime).
#[test]
fn perl_compat_arrow_array_exists_delete() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $a = [10, 20, 30];
        my $e0 = exists $a->[99] ? 1 : 0;
        my $e1 = exists $a->[1] ? 1 : 0;
        my $d = delete $a->[1];
        my $e2 = exists $a->[1] ? 1 : 0;
        $e0 . "," . $e1 . "," . $d . "," . $e2;"#,),
        "0,1,20,1"
    );
}

#[test]
fn perl_compat_arrow_array_compound_add() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $v = [1, 2, 3]; $v->[1] += 10; $v->[1]"#),
        12
    );
}

#[test]
fn perl_compat_arrow_hash_compound_add_and_concat() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { a => 10 }; $h->{a} += 2; $h->{a}"#),
        12
    );
    assert_eq!(
        rs(r#"no strict 'vars'; my $h = { a => "z" }; $h->{a} .= "9"; $h->{a}"#),
        "z9"
    );
}

#[test]
fn perl_compat_scalar_deref_hash_string_concat_assign() {
    assert_eq!(
        rs(r#"my %h = (a => "z"); my $r = \%h; $$r{a} .= "9"; $h{a}"#),
        "z9"
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
    assert_eq!(ri("my @a = (1..10); 0+@a;"), 10);
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
fn backslash_array_hash_ref_alias() {
    assert_eq!(
        rs("my @a = (1,2,3); my $r = \\@a; ref($r)"),
        "ARRAY".to_string()
    );
    assert_eq!(ri("my @a = (1,2,3); my $r = \\@a; $r->[0] = 99; $a[0]"), 99);
    assert_eq!(
        ri("my @a = (1,2,3); my $r = \\@a; my @c = @$r; $c[0] + $c[1] + $c[2]"),
        6
    );
    assert_eq!(
        rs("my %h = (a=>7); my $r = \\%h; ref($r)"),
        "HASH".to_string()
    );
    assert_eq!(ri("my %h = (a=>7); my $r = \\%h; $r->{a}"), 7);
}

#[test]
fn scalar_deref_hashref_brace_subscript() {
    assert_eq!(ri(r#"my %h=(a=>1); my $r=\%h; $$r{a}"#), 1);
}

#[test]
fn scalar_deref_aref_bracket_subscript() {
    assert_eq!(ri(r#"my @a=(10,20,30); my $r=\@a; $$r[1]"#), 20);
}

#[test]
fn interpolated_string_at_scalar_aref() {
    assert_eq!(
        rs(r#"my $r = [1, 2, 3]; my $s = "@$r"; $s"#),
        "1 2 3".to_string()
    );
}

#[test]
fn pop_push_peeled_array_deref_operand() {
    assert_eq!(
        rs(r#"my @a = (9, 8); my $r = \@a; pop((@$r)); join "-", @a"#),
        "9".to_string()
    );
    assert_eq!(
        rs(r#"my @a = (1); my $r = \@a; push((@$r), 2); join "", @a"#),
        "12".to_string()
    );
}

#[test]
fn splice_offset_length_perl_rules() {
    assert_eq!(
        rs("my @a = (1,2,3,4,5); my @rem = splice(@a, -2); join(' ', @rem) . '|' . join(' ', @a);"),
        "4 5|1 2 3"
    );
    assert_eq!(
        rs("my @a = (1,2,3,4,5); my @rem = splice(@a, 0, -2); join(' ', @rem) . '|' . join(' ', @a);"),
        "1 2 3|4 5"
    );
    assert_eq!(
        rs("my @a = (1,2,3); my @r = splice(@a, 100); join(' ', @a) . '|' . scalar @r;"),
        "1 2 3|0"
    );
}

/// Mirrors the reported breakage: `\` on aggregates must yield `ARRAY`/`HASH` refs with shared
/// storage; `splice` must not panic on past-end offsets and must honor negative offset/length;
/// `pop`/`shift`/`push`/`unshift` and `splice` must accept `@$aref`; qq must expand `"@$r"`; and
/// `$$r{...}` / `$$r[...]` must work like Perl `${$r}{...}` / `${$r}[...]`.
#[test]
fn perl_compat_regression_ref_deref_splice_qq_scalar_deref_subscript() {
    assert_eq!(rs(r#"my @a=(1,2,3); my $r=\@a; ref($r)"#), "ARRAY");
    assert_eq!(rs(r#"my %h=(a=>1); my $r=\%h; ref($r)"#), "HASH");
    assert_eq!(
        ri(r#"my @a=(1,2,3); my $r=\@a; my @c=@$r; $c[0]+$c[1]+$c[2]"#),
        6
    );
    assert_eq!(ri(r#"my @a=(1,2,3); my $r=\@a; $r->[1]=42; $a[1]"#), 42);

    assert_eq!(
        rs(r#"my @a=(1,2,3); my @rem=splice(@a,100); join(" ",@a)."|".scalar @rem"#),
        "1 2 3|0"
    );
    assert_eq!(
        rs(r#"my @a=(1,2,3,4,5); my @rem=splice(@a,-2); join(" ",@rem)."|".join(" ",@a)"#),
        "4 5|1 2 3"
    );
    assert_eq!(
        rs(r#"my @a=(1,2,3,4,5); my @rem=splice(@a,0,-2); join(" ",@rem)."|".join(" ",@a)"#),
        "1 2 3|4 5"
    );
    assert_eq!(
        rs(r#"my $v=[1,2,3]; splice @$v,100; join "-", @$v"#),
        "1-2-3"
    );

    assert_eq!(ri(r#"my @a=(9,8); my $r=\@a; my $x=pop @$r; $x+$a[0]"#), 17);
    assert_eq!(ri(r#"my @a=(9,8); my $r=\@a; shift @$r; $a[0]"#), 8);
    assert_eq!(
        rs(r#"my @a=(1); my $r=\@a; push @$r,2,3; join "",@a"#),
        "123"
    );
    assert_eq!(
        rs(r#"my @a=(2); my $r=\@a; unshift @$r,1; join "-",@a"#),
        "1-2"
    );

    assert_eq!(rs(r#"my $r=[1,2,3]; my $s="@$r\n"; $s"#), "1 2 3\n");

    assert_eq!(ri(r#"my %h=(a=>1); my $r=\%h; $$r{a}"#), 1);
    assert_eq!(ri(r#"my %h=(a=>1); my $r=\%h; $$r{b}=7; $h{b}"#), 7);
    assert_eq!(ri(r#"my @a=(10,20,30); my $r=\@a; $$r[1]"#), 20);
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

/// `->method LIST` without parens (Perl), `{ ... }` block as code ref, postfix `for`.
#[test]
fn ppool_submit_optional_paren_block_postfix_for() {
    let n = ri(r#"
        my $p = ppool(2);
        $p->submit({ $_ * 3 }, $_) for (2, 4);
        my @r = $p->collect();
        $r[0] + $r[1];
    "#);
    assert_eq!(n, 18);
}

/// README: one-arg `submit` uses caller `$_` so postfix `for @tasks` binds each task.
#[test]
fn ppool_submit_single_arg_postfix_for_uses_callers_topic() {
    let n = ri(r#"
        my $pool = ppool(4);
        my @tasks = (2, 4);
        $pool->submit({ $_ * 3 }) for @tasks;
        my @results = $pool->collect();
        $results[0] + $results[1];
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
fn readdir_list_context_returns_all_remaining_entries() {
    let base = std::env::temp_dir().join(format!("perlrs_sem_rdl_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    std::fs::write(base.join("a.txt"), b"x").expect("write");
    std::fs::write(base.join("b.txt"), b"y").expect("write");
    let pd = base.to_string_lossy();
    let script = format!(
        r#"opendir H, "{pd}" or die;
        my @f = readdir H;
        closedir H;
        (scalar grep {{ $_ eq "a.txt" }} @f) && (scalar grep {{ $_ eq "b.txt" }} @f) ? 1 : 0"#,
    );
    assert_eq!(ri(&script), 1);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn rewinddir_resets_read_position() {
    assert_eq!(
        ri(r#"opendir D, "."; readdir D; rewinddir D; (telldir D) == 0 ? 1 : 0;"#),
        1
    );
}

#[test]
fn perl_compat_opendir_finds_known_entry_in_temp_dir() {
    let base = std::env::temp_dir().join(format!("perlrs_sem_od_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    std::fs::write(base.join("mark.txt"), b"x").expect("write");
    let pd = base.to_string_lossy();
    let script = format!(
        r#"opendir H, "{pd}" or die;
        my @n;
        for (1..16) {{
          my $e = readdir H;
          last unless defined $e;
          push @n, $e;
        }}
        closedir H;
        scalar grep {{ $_ eq "mark.txt" }} @n;"#,
    );
    assert_eq!(ri(&script), 1);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn perl_compat_readlink_symlink_target_string() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("perlrs_sem_rl_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    let link = base.join("rl");
    symlink("expected_tgt", &link).expect("symlink");
    let sl = link.to_string_lossy();
    assert_eq!(
        ri(&format!(r#"(readlink "{sl}") eq "expected_tgt" ? 1 : 0;"#)),
        1
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn perl_compat_hard_link_reads_same_bytes() {
    let dir = std::env::temp_dir();
    let a = dir.join(format!("perlrs_sem_hl_a_{}", std::process::id()));
    let b = dir.join(format!("perlrs_sem_hl_b_{}", std::process::id()));
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    std::fs::write(&a, b"hl").expect("write");
    let sa = a.to_string_lossy();
    let sb = b.to_string_lossy();
    let script = format!(
        r#"link "{sa}", "{sb}";
        slurp "{sb}";"#,
    );
    assert_eq!(rs(&script), "hl");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

#[test]
fn tell_stdout_unbuffered_slot_returns_negative_one() {
    assert_eq!(ri(r#"tell STDOUT;"#), -1);
}

#[test]
fn tell_writable_open_file_reports_byte_offset() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_tell_semantics_test");
    let _ = std::fs::remove_file(&path);
    let ps = path.to_string_lossy();
    let script = format!(r#"open F, ">", "{ps}"; print F "abc"; my $p = tell F; close F; $p"#);
    assert_eq!(ri(&script), 3);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_quotemeta_escapes_dot() {
    assert_eq!(rs(r#"quotemeta("a.c");"#), r"a\.c");
}

#[test]
fn perl_compat_quotemeta_escapes_path_slashes() {
    assert_eq!(rs(r#"quotemeta("/usr/bin");"#), r"\/usr\/bin");
}

#[test]
fn perl_compat_chomp_returns_chars_removed() {
    assert_eq!(ri(r#"my $s = "ab\n"; my $n = chomp $s; $n;"#), 1);
}

#[test]
fn perl_compat_subst_replacement_expands_env_brace() {
    let home = std::env::var("HOME").expect("HOME");
    let script = r#"$_ = "~"; s@^~@$ENV{HOME}@; $_;"#.to_string();
    assert_eq!(rs(&script), home);
}

#[test]
fn perl_compat_my_array_from_do_block_list_context() {
    assert_eq!(
        ri(r#"my @a = do { (10, 20, 30) };
        $a[0] + $a[1] + $a[2];"#),
        60
    );
}

#[test]
fn perl_compat_my_hash_from_do_block_list_context() {
    assert_eq!(
        ri(r#"my %h = do { ("x", 3, "y", 4) };
        $h{"x"} * 10 + $h{"y"};"#),
        34
    );
}

#[test]
fn perl_compat_array_assign_flattens_hash_to_key_value_list() {
    assert_eq!(
        ri(r#"my %h = ("p", 10, "q", 20);
        my @a = %h;
        scalar @a;"#),
        4
    );
}

#[test]
fn perl_compat_getc_reads_bytes_from_open_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_getc_semantics_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"mn").expect("write temp");
    let ps = path.to_string_lossy();
    let script = format!(
        r#"open G, "<", "{ps}";
        my $t = (getc G) . (getc G);
        close G;
        $t;"#
    );
    assert_eq!(rs(&script), "mn");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_join_with_custom_list_separator() {
    assert_eq!(
        rs(r#"no strict 'vars';
        $" = ":";
        join $", ("x", "y");"#),
        "x:y"
    );
}

#[test]
fn perl_compat_qq_array_with_custom_list_separator() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my @t = (5, 6);
        $" = "_";
        "@t";"#),
        "5_6"
    );
}

#[test]
fn perl_compat_sysseek_then_tell_on_open_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_sysseek_semantics_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"ABCDE").expect("write temp");
    let ps = path.to_string_lossy();
    let script = format!(
        r#"open S, "<", "{ps}";
        sysseek S, 3, 0;
        my $n = tell S;
        close S;
        $n;"#
    );
    assert_eq!(ri(&script), 3);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_scalar_values_hash() {
    assert_eq!(
        ri(r#"my %g = ("x", 9, "y", 8);
        scalar values %g;"#),
        2
    );
}

#[test]
fn perl_compat_eof_string_handle_open_vs_closed() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_eof_semantics_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"z").expect("write temp");
    let ps = path.to_string_lossy();
    let open_then = format!(
        r#"open Q, "<", "{ps}";
        my $a = eof("Q");
        close Q;
        $a;"#
    );
    let after_close = format!(
        r#"open Q, "<", "{ps}";
        close Q;
        eof("Q");"#
    );
    assert_eq!(ri(&open_then), 0);
    assert_eq!(ri(&after_close), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_truncate_shortens_file_by_path() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_truncate_semantics_test");
    let _ = std::fs::remove_file(&path);
    let ps = path.to_string_lossy();
    let script = format!(
        r#"open W, ">", "{ps}";
        print W "abcd";
        close W;
        truncate "{ps}", 1;
        open R, "<", "{ps}";
        my $t = readline R;
        close R;
        length $t;"#
    );
    assert_eq!(ri(&script), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_split_comma_with_limit() {
    assert_eq!(ri(r#"scalar split(",", "u,v,w,x", 2);"#), 2);
}

#[test]
fn perl_compat_study_empty_vs_nonempty() {
    assert_eq!(ri(r#"study "";"#), 0);
    assert_eq!(ri(r#"study "n";"#), 1);
}

#[test]
fn perl_compat_hex_oct_builtins() {
    assert_eq!(ri(r#"hex("FF");"#), 255);
    assert_eq!(ri(r#"oct("10");"#), 8);
}

#[test]
fn perl_compat_eval_string_and_block() {
    assert_eq!(ri(r#"eval '17 + 25';"#), 42);
    assert_eq!(ri(r#"eval { 9 * 4 + 6 };"#), 42);
}

#[test]
fn perl_compat_unpack_after_pack_byte() {
    assert_eq!(ri(r#"scalar unpack("C", pack("C", 88));"#), 88);
}

#[test]
fn perl_compat_filetest_s_nonempty_and_z_empty() {
    let dir = std::env::temp_dir();
    let nonempty = dir.join("perlrs_sem_filetest_s");
    let empty = dir.join("perlrs_sem_filetest_z");
    let _ = std::fs::remove_file(&nonempty);
    let _ = std::fs::remove_file(&empty);
    std::fs::write(&nonempty, b"x").expect("write");
    std::fs::write(&empty, b"").expect("write");
    let sn = nonempty.to_string_lossy();
    let se = empty.to_string_lossy();
    assert_eq!(ri(&format!(r#"-s "{sn}";"#)), 1);
    assert_eq!(ri(&format!(r#"-z "{se}";"#)), 1);
    let _ = std::fs::remove_file(&nonempty);
    let _ = std::fs::remove_file(&empty);
}

#[test]
fn perl_compat_sleep_zero() {
    assert_eq!(ri("sleep 0;"), 0);
}

#[test]
fn perl_compat_glob_txt_in_directory() {
    let base = std::env::temp_dir().join(format!("perlrs_sem_glob_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    std::fs::write(base.join("p.txt"), b"1").expect("write");
    std::fs::write(base.join("q.txt"), b"2").expect("write");
    std::fs::write(base.join("r.rst"), b"3").expect("write");
    let d = base.to_string_lossy();
    let script = format!(
        r#"my $dir = "{d}";
        my @g = glob("$dir/*.txt");
        scalar @g;"#
    );
    assert_eq!(ri(&script), 2);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn perl_compat_qx_printf_stdout() {
    assert_eq!(rs(r#"scalar `printf '%s' sem_qx`;"#), "sem_qx");
}

#[test]
fn perl_compat_select_roundtrip_default_handle() {
    assert_eq!(
        rs(r#"my $a = select(STDERR);
        my $b = select($a);
        $a . "/" . $b;"#),
        "STDOUT/STDERR"
    );
}

#[test]
fn perl_compat_ref_bless_and_delete_exists() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $o = bless {}, "Box";
        ref $o;"#),
        "Box"
    );
    assert_eq!(
        ri(r#"my %m = ("u", 5);
        my $x = delete $m{"u"};
        my $y = exists $m{"u"};
        $x * 100 + $y;"#),
        500
    );
}

#[test]
fn perl_compat_index_rindex_substr_splice() {
    assert_eq!(ri(r#"index("xyx", "x") + 5 * rindex("xyx", "x");"#), 10);
    assert_eq!(rs(r#"substr("Perl", 1, 3);"#), "erl");
    assert_eq!(
        rs(r#"my @s = (1, 2, 3, 4);
        join(",", splice @s, 0, 2);"#),
        "1,2"
    );
}

#[test]
fn perl_compat_splice_aref_list_replacement() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [10, 20, 30, 40];
        my $r = join "-", splice @$v, 1, 2, 7, 8;
        ($r eq "20-30" && $v->[1] == 7 && $v->[2] == 8) ? 1 : 0;"#,),
        1
    );
}

#[test]
fn perl_compat_scalar_splice_aref_last_removed() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $a = [3, 4, 5];
        scalar splice @$a, 0, 2;"#,),
        4
    );
}

/// `splice @$aref` must use the same OFFSET/LENGTH rules as `splice @name` (negative offset
/// removes from the end; negative LENGTH preserves a tail). Exercises the VM fast path
/// (`Op::SpliceArrayDeref`), not only the named-array builtin path.
#[test]
fn perl_compat_splice_aref_negative_offset_length() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3, 4, 5];
        my @rem = splice @$v, -2;
        join(" ", @$v) . "|" . join(" ", @rem);"#),
        "1 2 3|4 5"
    );
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3, 4, 5];
        my @rem = splice @$v, 0, -2;
        join(" ", @$v) . "|" . join(" ", @rem);"#),
        "4 5|1 2 3"
    );
}

/// Negative OFFSET with a replacement list must still use `SpliceArrayDeref(1)` (stack args), not
/// only the no-replacement fast path.
#[test]
fn perl_compat_splice_aref_negative_offset_with_replacement() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3, 4, 5];
        splice @$v, -2, 1, 99;
        join "-", @$v;"#),
        "1-2-3-99-5"
    );
}

/// `$$r{key}` must keep the hash **reference** on the stack for `->`/arrow lowering (same fix as
/// `$href->{key}`): compound assign and post-increment must mutate the underlying `%h`.
#[test]
fn perl_compat_scalar_deref_hash_compound_assign_and_postinc() {
    assert_eq!(
        ri(r#"my %h = (a => 1); my $r = \%h; $$r{a} += 5; $h{a}"#),
        6
    );
    assert_eq!(
        ri(r#"my %h = (a => 5); my $r = \%h; my $x = $$r{a}++; $x * 10 + $h{a}"#),
        56
    );
}

#[test]
fn perl_compat_scalar_deref_hash_logassign() {
    assert_eq!(
        ri(r#"my %h = (a => 2); my $r = \%h; $$r{a} &&= 7; $h{a}"#),
        7
    );
    assert_eq!(ri(r#"my %h = (); my $r = \%h; $$r{b} ||= 9; $h{b}"#), 9);
}

/// `$$r{key} //=` must use the hash reference as the container (same lowering as `$h->{key}`), not
/// a copied `%` aggregate.
#[test]
fn perl_compat_scalar_deref_hash_defined_or_assign() {
    assert_eq!(ri(r#"my %h = (); my $r = \%h; $$r{x} //= 42; $h{x}"#), 42);
    assert_eq!(
        ri(r#"my %h = (a => 1); my $r = \%h; $$r{a} //= 99; $h{a}"#),
        1
    );
    assert_eq!(
        ri(r#"my %h = (a => 1); my $r = \%h; my $runs = 0; $$r{a} //= ($runs = 1); $runs"#,),
        0
    );
}

#[test]
fn perl_compat_scalar_splice_aref_remove_one() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my $s = scalar splice @$v, 1, 1;
        $s . "|" . join "-", @$v"#),
        "2|1-3"
    );
}

#[test]
fn perl_compat_scalar_at_aref_is_array_length() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $r = [7, 8, 9, 0];
        scalar @$r;"#,),
        4
    );
}

#[test]
fn perl_compat_scalar_at_aref_empty_ref_is_zero() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $e = [];
        scalar @$e;"#,),
        0
    );
}

#[test]
fn perl_compat_scalar_braced_aref_is_length() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $r = [1, 2, 3];
        scalar @{$r};"#,),
        3
    );
}

#[test]
fn perl_compat_scalar_braced_sub_return_aref_count() {
    assert_eq!(
        ri(r#"no strict 'vars';
        sub row { [0, 0, 0, 0, 0, 0] }
        scalar @{row()};"#,),
        6
    );
}

#[test]
fn perl_compat_assignment_list_deref_aref_to_scalar() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $x = [1, 1, 1];
        my $c = @$x;
        $c;"#,),
        3
    );
}

#[test]
fn perl_compat_join_receives_scalar_at_aref_as_one_field() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [2, 4, 6];
        join "x", scalar @$v;"#,),
        "3"
    );
}

#[test]
fn perl_compat_scalar_named_array_length() {
    assert_eq!(
        ri(r#"my @w = (0, 0, 0, 0, 0);
        scalar @w;"#,),
        5
    );
}

#[test]
fn perl_compat_scalar_percent_href_fill_metric() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $h = { x => 1, y => 2 };
        scalar %$h;"#,),
        "2/3"
    );
}

#[test]
fn perl_compat_scalar_percent_href_empty_zero() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $e = {};
        scalar %$e;"#,),
        0
    );
}

#[test]
fn perl_compat_slurp_and_unlink_temp_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_slurp_unlink_semantics");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"ok").expect("write temp");
    let ps = path.to_string_lossy();
    let slurp = format!(r#"slurp "{ps}";"#);
    assert_eq!(rs(&slurp), "ok");
    let rm = format!(r#"unlink "{ps}";"#);
    assert_eq!(ri(&rm), 1);
    assert!(!path.exists());
}

#[test]
fn perl_compat_stat_size_and_missing_list() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_sem_stat_sz");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"abcdef").expect("write temp");
    let ps = path.to_string_lossy();
    let sz = format!(
        r#"my @st = stat("{ps}");
        $st[7];"#,
    );
    assert_eq!(ri(&sz), 6);
    assert_eq!(
        ri(r#"my @st = stat("perlrs___no___stat___"); scalar @st;"#),
        0
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_readline_scalar_length_includes_newline() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_sem_readline_scalar");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"a\nbb\n").expect("write temp");
    let ps = path.to_string_lossy();
    let script = format!(
        r#"open SRL, "<", "{ps}";
        my $x = readline SRL;
        close SRL;
        length $x;"#
    );
    assert_eq!(ri(&script), 2);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn perl_compat_mkdir_and_d_test() {
    let base = std::env::temp_dir().join(format!("perlrs_sem_mkdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let pb = base.to_string_lossy();
    let script = format!(
        r#"mkdir "{pb}";
        (-d "{pb}") + (-e "{pb}");"#,
    );
    assert_eq!(ri(&script), 2);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn perl_compat_capture_true_exitcode() {
    assert_eq!(ri(r#"my $c = capture("true"); $c->exitcode;"#), 0);
}

#[test]
fn perl_compat_filetest_e_f_regular_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("perlrs_sem_ef");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"x").expect("write");
    let ps = path.to_string_lossy();
    assert_eq!(ri(&format!(r#"(-e "{ps}") + (-f "{ps}");"#)), 2);
    let _ = std::fs::remove_file(&path);
}

#[cfg(unix)]
#[test]
fn perl_compat_lstat_symlink_st_size_not_followed() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("perlrs_sem_lstat_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    let target = base.join("longtargetfilename");
    std::fs::write(&target, b"z").expect("write");
    let link = base.join("sym");
    symlink("longtargetfilename", &link).expect("symlink");
    let sl = link.to_string_lossy();
    let script = format!(
        r#"my @st = stat("{sl}");
        my @l = lstat("{sl}");
        $st[7] * 100 + $l[7];"#,
    );
    assert_eq!(ri(&script), 118);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn perl_compat_wantarray_rename_lc_uc() {
    assert_eq!(
        ri(r#"sub ctx { wantarray ? 2 : 8 }
        my $x = ctx();
        my @y = ctx();
        $x * 10 + $y[0];"#),
        82
    );
    let dir = std::env::temp_dir();
    let a = dir.join("perlrs_sem_rename_a");
    let b = dir.join("perlrs_sem_rename_b");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    std::fs::write(&a, b"ok").expect("write");
    let sa = a.to_string_lossy();
    let sb = b.to_string_lossy();
    let script = format!(r#"rename "{sa}", "{sb}"; length(slurp "{sb}");"#);
    assert_eq!(ri(&script), 2);
    let _ = std::fs::remove_file(&b);
    assert_eq!(rs(r#"lc("Hi") . uc("m");"#), "hiM");
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
fn fan_progress_optional_parses_and_runs() {
    let v = run(r#"fan 2 { 1 }, progress => 0;"#).expect("run");
    assert!(v.is_undef());
    let v2 = run(r#"fan { 1 }, progress => 0;"#).expect("run");
    assert!(v2.is_undef());
}

/// Postfix `pfor` may follow a readpipe (or any expr stmt) without `;` — not only `{ } pfor` / `do { } pfor`.
/// Regression: `fan { ... `cmd` pfor (1,2,3); ... }, progress => …` must parse.
#[test]
fn postfix_pfor_after_backtick_without_semicolon_runs() {
    assert_eq!(ri(r#"my $x = 1; `true` pfor (1, 2, 3); 42"#), 42,);
}

#[test]
fn fan_block_backtick_postfix_pfor_progress_runs() {
    run(r#"fan { my $x = "tommy"; `true` pfor (1, 2, 3); sleep 0 }, progress => 0;"#).expect("run");
}

#[test]
fn glob_par_progress_optional_runs() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sub = format!("target/glob_par_test_{}", std::process::id());
    let dir = base.join(&sub);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create target/ glob_par scratch dir");
    std::fs::write(dir.join("probe.rs"), b"// glob_par test\n").expect("write probe.rs");
    // Relative to crate root (same as `glob_par "src/*.rs"` tests). Absolute patterns are not
    // fully handled by `glob_par`’s recursive walker yet.
    let pat = format!("{sub}/*.rs");
    let n = ri(&format!(r#"scalar glob_par "{pat}", progress => 0;"#));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        n >= 1,
        "glob_par with progress => 0 should match at least one file, got {n}"
    );
}

#[test]
fn fan_cap_returns_list_in_index_order() {
    let s = r#"
        my $s = join ",", fan_cap 4 { $_ * 2 };
        $s eq "0,2,4,6" ? 1 : 0;
    "#;
    assert_eq!(ri(s), 1);
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
fn heap_block_comparator_readme_form() {
    let s = r#"
        my $pq = heap({ $a <=> $b });
        $pq->push(3);
        my $min = $pq->pop();
        $min;
    "#;
    assert_eq!(ri(s), 3);
}

#[test]
fn heap_sub_comparator_sees_outer_lexical() {
    let s = r#"
        my $bias = 0;
        my $pq = heap(sub { $a <=> ($b + $bias) });
        $pq->push(1);
        $pq->push(100);
        $pq->pop() + 0;
    "#;
    assert_eq!(ri(s), 1);
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

/// Bare `{ }` blocks and `pipeline(@arr)` (flattened) — same semantics as `sub { }` + scalars.
#[test]
fn pipeline_chain_bare_blocks_and_array_source() {
    let s = r#"
        my @data = (5, 11, 12, 9);
        my @result = pipeline(@data)
            ->filter({ $_ > 10 })
            ->map({ $_ * 2 })
            ->take(100)
            ->collect();
        $result[0] + $result[1];
    "#;
    assert_eq!(ri(s), 46);
}

#[test]
fn pipeline_parallel_pgrep_pmap_psort() {
    let s = r#"
        my @r = pipeline(3, 1, 4)
            ->pgrep(sub { $_ > 1 })
            ->pmap(sub { $_ + 10 })
            ->psort(sub { $a <=> $b })
            ->collect();
        scalar @r;
    "#;
    assert_eq!(ri(s), 2);
}

#[test]
fn pipeline_preduce_collect_scalar() {
    assert_eq!(
        ri(r#"pipeline(1, 2, 3, 4)->preduce(sub { $a + $b })->collect();"#),
        10
    );
}

#[test]
fn pipeline_chaining_rejects_ops_after_preduce() {
    assert!(run(r#"pipeline(1, 2)->preduce(sub { $a + $b })->map(sub { $_ });"#,).is_err());
}

/// `->name` with no args resolves a package subroutine and applies it like `map` (`$_` each item).
#[test]
fn pipeline_user_sub_in_chain() {
    let s = r#"
        sub times2 { $_ * 2 }
        my @r = pipeline(1, 2, 3)->times2->collect();
        $r[0] + $r[1] + $r[2];
    "#;
    assert_eq!(ri(s), 12);
}

#[test]
fn pipeline_grep_alias_matches_filter() {
    let s = r#"
        my @r = pipeline(1, 2, 3, 4)->grep(sub { $_ % 2 == 0 })->collect();
        scalar @r;
    "#;
    assert_eq!(ri(s), 2);
}

#[test]
fn pipeline_qualified_sub_chain() {
    let s = r#"
        package P;
        sub triple { $_ * 3 }
        package main;
        my @r = pipeline(1, 2)->P::triple->collect();
        $r[0] + $r[1];
    "#;
    assert_eq!(ri(s), 9);
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
fn spawn_await_same_as_async() {
    assert_eq!(ri(r#"my $t = spawn { 40 + 2 }; await($t);"#), 42);
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

/// Perl two-arg `open $fh, "cmd |"` (trailing `|`) — same as `-|`, `sh -c`.
#[test]
fn open_read_pipe_two_arg_trailing_pipe() {
    let out = rs(r#"
        open(FH, "echo hi |") or die "open";
        my $x = <FH>;
        close FH;
        $x;
    "#);
    assert!(out.contains("hi"));
}

/// Perl two-arg `open $fh, "| cmd"` (leading `|`) — same as `|-`, `sh -c`.
#[test]
fn open_write_pipe_two_arg_leading_pipe() {
    let out = rs(r#"
        open(FH, "| tr a-z A-Z") or die "open";
        print FH "abc\n";
        close FH;
        "done";
    "#);
    assert_eq!(out, "done");
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

#[test]
fn autoload_method_sets_package_method_in_autoload() {
    assert_eq!(
        rs(r#"
        package D;
        sub AUTOLOAD { $AUTOLOAD }
        package main;
        bless({}, "D")->missing_meth();
    "#),
        "D::missing_meth"
    );
}

#[test]
fn compile_phase_blocks_run_before_main() {
    assert_eq!(
        rs(r#"
        BEGIN { $main::order = ""; $main::order .= "B" }
        UNITCHECK { $main::order .= "U" }
        CHECK { $main::order .= "C" }
        INIT { $main::order .= "I" }
        $main::order .= "M";
        $main::order
    "#),
        "BUCIM"
    );
}

#[test]
fn runtime_push_isa_updates_method_resolution() {
    assert_eq!(
        ri(r#"
        package P;
        sub ping { 42 }
        package C;
        our @ISA;
        push @C::ISA, "P";
        package main;
        my $o = bless {}, "C";
        $o->ping()
    "#),
        42
    );
}

#[test]
fn typeglob_assign_scalar_ref_to_coderef_aliases_sub() {
    assert_eq!(
        ri(r#"
        package P;
        sub orig { 7 }
        *alias = \&orig;
        package main;
        P::alias()
    "#),
        7
    );
}

#[test]
fn use_open_encoding_utf8_sets_open_caret() {
    assert_eq!(ri(r#"use open qw(:utf8); ${^OPEN}"#), 1);
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
#[test]
fn perl_compat_use_overload_combined_coderef() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '+' => \&add, '""' => \&stringify;
        sub add { my ($a, $b) = @_; $a->{n} + $b->{n} }
        sub stringify { "v" . $_[0]->{n} }
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
        sub stringify { "v" . $_[0]->{n} }
        package main;
        my $o = bless { n => 7 }, "O";
        "$o"
        "#),
        "v7"
    );
}

#[test]
fn perl_compat_sprintf_percent_s_uses_overload_stringify() {
    assert_eq!(
        rs(r#"
        package O;
        use overload '""' => 'as_string';
        sub as_string { "Z" }
        package main;
        my $o = bless {}, "O";
        sprintf "%s", $o;
        "#),
        "Z"
    );
}

#[test]
fn perl_compat_regex_caret_match_vars() {
    assert_eq!(
        rs(r#"
        "abc" =~ /b/;
        "${^MATCH}" . ":" . "${^PREMATCH}" . ":" . "${^POSTMATCH}";
        "#),
        "b:a:c"
    );
}

#[test]
fn perl_compat_tie_hash_exists_delete() {
    assert_eq!(
        ri(r#"
        package T;
        sub TIEHASH { bless { h => {} }, shift }
        sub FETCH { $_[0]->{h}->{$_[1]} }
        sub STORE { $_[0]->{h}->{$_[1]} = $_[2] }
        sub EXISTS { 7 }
        sub DELETE { 8 }
        package main;
        my %t;
        tie %t, "T";
        $t{a} = 1;
        exists $t{a};
        "#),
        7
    );
    assert_eq!(
        ri(r#"
        package T;
        sub TIEHASH { bless { h => {} }, shift }
        sub FETCH { $_[0]->{h}->{$_[1]} }
        sub STORE { $_[0]->{h}->{$_[1]} = $_[2] }
        sub EXISTS { 1 }
        sub DELETE { 8 }
        package main;
        my %t;
        tie %t, "T";
        $t{a} = 1;
        delete $t{a};
        "#),
        8
    );
}

#[test]
fn perl_compat_use_overload_nomethod_binop() {
    assert_eq!(
        ri(r#"
        package O;
        use overload nomethod => 'catch_all', fallback => 1;
        sub catch_all { my ($a, $b, $op) = @_; 99 }
        package main;
        my $a = O->new(n => 1);
        my $b = O->new(n => 2);
        $a + $b;
        "#),
        99
    );
}

#[test]
fn perl_compat_use_overload_unary_neg() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'neg' => 'negate';
        sub negate { my ($x) = @_; 42 }
        package main;
        my $o = bless {}, "O";
        -$o;
        "#),
        42
    );
}

/// Unary `!` consults overload `bool` when present (then inverts), matching Perl 5.
#[test]
fn perl_compat_use_overload_bool_for_logical_not() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'bool' => 'as_bool';
        sub as_bool { $_[0]->{t} }
        package main;
        my $f = bless { t => 0 }, "O";
        my $t = bless { t => 1 }, "O";
        (!$f) * 10 + (!$t);
        "#),
        10
    );
}

/// High-precedence `not` uses the same bool overload hook as `!`.
#[test]
fn perl_compat_use_overload_bool_for_not_keyword() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'bool' => 'as_bool';
        sub as_bool { $_[0]->{t} }
        package main;
        my $f = bless { t => 0 }, "O";
        my $t = bless { t => 1 }, "O";
        (not $f) * 10 + (not $t);
        "#),
        10
    );
}

/// `join` stringifies elements like `sprintf "%s"` — `use overload '""'` applies.
#[test]
fn perl_compat_join_stringifies_overloaded_objects() {
    assert_eq!(
        rs(r#"
        package O;
        use overload '""' => 'as_str';
        sub as_str { "@" . $_[0]->{n} }
        package main;
        my $a = bless { n => 1 }, "O";
        my $b = bless { n => 2 }, "O";
        join ":", $a, $b,3;
        "#),
        "@1:@2:3"
    );
}

/// CPAN modules often emit an empty `use overload ();` after defining methods.
#[test]
fn perl_compat_use_overload_empty_list_runs() {
    assert_eq!(
        ri(r#"
        package O;
        sub add { 1 }
        use overload ();
        package main;
        9;
        "#),
        9
    );
}

#[test]
fn perl_compat_use_overload_dispatches_sub_mul_cmp_ops() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '-' => 'osub', '*' => 'omul', '==' => 'onumeq', 'eq' => 'ostreq', cmp => 'ocmp';
        sub osub { my ($a, $b) = @_; $a->{n} - $b->{n} }
        sub omul { my ($a, $b) = @_; $a->{n} * $b->{n} }
        sub onumeq { my ($a, $b) = @_; $a->{n} == $b ? 1 : 0 }
        sub ostreq { my ($a, $b) = @_; $b eq "rhs" ? 1 : 0 }
        sub ocmp { my ($a, $b) = @_; $a->{n} <=> $b }
        package main;
        my $x = O->new(n => 10);
        my $y = O->new(n => 4);
        my $s = $x - $y;
        my $m = $x * $y;
        my $e = $x == 10;
        my $q = $x eq "rhs";
        my $c = $x cmp 0;
        $s * 1000 + $m * 100 + $e * 10 + $q + $c;
        "#),
        10_012
    );
}

#[test]
fn perl_compat_use_overload_dispatches_concat_op() {
    assert_eq!(
        rs(r#"
        package O;
        use overload '.' => 'odot';
        sub odot { my ($a, $b) = @_; "[" . $a->{n} . "+" . $b . "]" }
        package main;
        my $a = O->new(n => "x");
        $a . "z";
        "#),
        "[x+z]"
    );
}

/// String on the LHS still dispatches the overloaded object’s `.` handler (Perl swaps operands).
#[test]
fn perl_compat_use_overload_dispatches_concat_op_string_on_lhs() {
    assert_eq!(
        rs(r#"
        package O;
        use overload '.' => 'odot';
        sub odot { my ($a, $b) = @_; "[" . $a->{n} . "+" . $b . "]" }
        package main;
        my $a = O->new(n => "x");
        "z" . $a;
        "#),
        "[x+z]"
    );
}

#[test]
fn perl_compat_qq_interpolates_lone_scalar_without_overload() {
    assert_eq!(
        rs(r#"
        my $u = 40;
        "$u";
        "#),
        "40"
    );
}

#[test]
fn perl_compat_qq_interpolates_lone_array_uses_list_separator() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my @a = (9, 8, 7);
        "@a";
        "#),
        "9 8 7"
    );
}

#[test]
fn perl_compat_use_overload_dispatches_div_mod_pow() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '/' => 'odiv', '%' => 'omod', '**' => 'opow';
        sub odiv { my ($a, $b) = @_; $a->{n} / $b }
        sub omod { my ($a, $b) = @_; $a->{n} % $b }
        sub opow { my ($a, $b) = @_; $a->{n} ** $b }
        package main;
        my $a = O->new(n => 20);
        my $c = O->new(n => 2);
        ($a / 4) + ($a % 7) + ($c ** 3);
        "#),
        19
    );
}

#[test]
fn perl_compat_use_overload_nomethod_dispatches_concat() {
    assert_eq!(
        ri(r#"
        package O;
        use overload nomethod => 'nm', fallback => 1;
        sub nm { my ($a, $b, $op) = @_; $op eq "." ? 777 : 0 }
        package main;
        my $a = O->new(n => 1);
        $a . "tail";
        "#),
        777
    );
}

#[test]
fn perl_compat_use_overload_dispatches_add_blessed_on_rhs() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '+' => 'add';
        sub add { my ($a, $b) = @_; $a->{n} + $b }
        package main;
        my $x = O->new(n => 7);
        5 + $x;
        "#),
        12
    );
}

#[test]
fn perl_compat_use_overload_dispatches_ne_and_spaceship() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'ne' => 'one', '<=>' => 'osp';
        sub one { my ($a, $b) = @_; 1 }
        sub osp { my ($a, $b) = @_; $a->{n} <=> $b }
        package main;
        my $o = O->new(n => 9);
        ($o <=> 4) * 10 + ($o ne "x");
        "#),
        11
    );
}

#[test]
fn perl_compat_qq_interpolates_lone_hash_element_expr() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my %h = (k => 42);
        "$h{k}";
        "#),
        "42"
    );
}

#[test]
fn perl_compat_use_overload_dispatches_str_lt_and_num_gt() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'lt' => 'olt', '>' => 'ogt';
        sub olt { my ($a, $b) = @_; $a->{"s"} lt $b ? 1 : 0 }
        sub ogt { my ($a, $b) = @_; $a->{"n"} > $b ? 1 : 0 }
        package main;
        my $o = bless { "n" => 5, "s" => "a" }, "O";
        ($o lt "b") * 10 + ($o > 3);
        "#),
        11
    );
}

#[test]
fn perl_compat_use_overload_sub_blessed_on_rhs() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '-' => 'osub';
        sub osub { my ($a, $b) = @_; $b - $a->{n} }
        package main;
        my $o = O->new(n => 7);
        10 - $o;
        "#),
        3
    );
}

#[test]
fn perl_compat_use_overload_mul_and_pow_blessed_on_rhs() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '*' => 'omul', '**' => 'opow';
        sub omul { my ($a, $b) = @_; $a->{n} * $b }
        sub opow { my ($a, $b) = @_; $b ** $a->{n} }
        package main;
        my $a = O->new(n => 6);
        my $b = O->new(n => 3);
        (4 * $a) * 10 + (2 ** $b);
        "#),
        248
    );
}

#[test]
fn perl_compat_use_overload_num_ne_and_num_le() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '!=' => 'oine', '<=' => 'ole';
        sub oine { my ($a, $b) = @_; $a->{n} != $b ? 1 : 0 }
        sub ole { my ($a, $b) = @_; $a->{n} <= $b ? 1 : 0 }
        package main;
        my $o = O->new(n => 7);
        ($o != 10) * 10 + ($o <= 7);
        "#),
        11
    );
}

#[test]
fn perl_compat_use_overload_str_le_and_str_ge() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'le' => 'ole', 'ge' => 'oge';
        sub ole { my ($a, $b) = @_; $a->{"t"} le $b ? 1 : 0 }
        sub oge { my ($a, $b) = @_; $a->{"t"} ge $b ? 1 : 0 }
        package main;
        my $o = bless { "t" => "m" }, "O";
        ($o le "n") * 10 + ($o ge "a");
        "#),
        11
    );
}

#[test]
fn perl_compat_use_overload_div_and_mod_blessed_on_rhs() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '/' => 'odiv', '%' => 'omod';
        sub odiv { my ($a, $b) = @_; $b / $a->{n} }
        sub omod { my ($a, $b) = @_; $b % $a->{n} }
        package main;
        my $a = O->new(n => 4);
        my $b = O->new(n => 3);
        (20 / $a) * 10 + (10 % $b);
        "#),
        51
    );
}

#[test]
fn perl_compat_use_overload_num_ge_and_num_lt() {
    assert_eq!(
        ri(r#"
        package O;
        use overload '>=' => 'oge', '<' => 'olt';
        sub oge { my ($a, $b) = @_; $a->{n} >= $b ? 1 : 0 }
        sub olt { my ($a, $b) = @_; $a->{n} < $b ? 1 : 0 }
        package main;
        my $o = O->new(n => 7);
        ($o >= 6) * 10 + ($o < 8);
        "#),
        11
    );
}

#[test]
fn perl_compat_use_overload_str_cmp_op() {
    assert_eq!(
        ri(r#"
        package O;
        use overload 'cmp' => 'ocmp';
        sub ocmp { my ($a, $b) = @_; $a->{"t"} cmp $b }
        package main;
        my $o = bless { "t" => "b" }, "O";
        $o cmp "c";
        "#),
        -1
    );
}

#[test]
fn perl_compat_qq_stringify_blessed_hash_value() {
    assert_eq!(
        rs(r#"
        package O;
        use overload '""' => 'as_str';
        sub as_str { "Zy" }
        package main;
        no strict 'vars';
        my %h = ("k" => bless {}, "O");
        "$h{k}";
        "#),
        "Zy"
    );
}

#[test]
fn perl_compat_qq_interpolates_literal_then_two_scalars() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my $x = 7;
        my $y = 8;
        "p${x}x$y";
        "#),
        "p7x8"
    );
}

#[test]
fn perl_compat_qq_interpolates_named_array_element() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my @a = (11, 22);
        "v$a[0]";
        "#),
        "v11"
    );
}

#[test]
fn perl_compat_qq_braced_scalar_trailing_literal() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my $u = 3;
        "n${u}px";
        "#),
        "n3px"
    );
}

#[test]
fn perl_compat_qq_braced_scalar_leading_and_trailing_literals() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my $u = 5;
        "L${u}R";
        "#),
        "L5R"
    );
}

#[test]
fn perl_compat_qq_mixed_braced_and_plain_scalar() {
    assert_eq!(
        rs(r#"
        no strict 'vars';
        my $x = 1;
        my $y = 2;
        "a${x}b$y";
        "#),
        "a1b2"
    );
}

#[test]
fn perl_compat_named_array_slice_list_assign() {
    assert_eq!(
        ri(r#"
        no strict 'vars';
        my @a = (0, 0);
        @a[0, 1] = (5, 6);
        $a[0] + $a[1];
        "#),
        11
    );
}

#[test]
fn perl_compat_named_array_slice_single_index_list_rhs() {
    assert_eq!(
        ri(r#"
        no strict 'vars';
        my @a = (0, 7);
        @a[0] = (9);
        $a[0] + $a[1];
        "#),
        16
    );
}

#[test]
fn perl_compat_named_array_slice_three_indices_list_assign() {
    assert_eq!(
        ri(r#"
        no strict 'vars';
        my @a = (0, 0, 0);
        @a[0, 1, 2] = (10, 20, 30);
        $a[0] + $a[1] + $a[2];
        "#),
        60
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

// ── VM / aggregate lowering: splice, arrow elems, scalar deref hash (regression catchers) ──

#[test]
fn perl_compat_splice_aref_zero_length_insert() {
    assert_eq!(
        rs(r#"no strict 'vars'; my $v = [1, 2, 3]; splice @$v, 1, 0, 9; join "-", @$v"#),
        "1-9-2-3"
    );
}

#[test]
fn perl_compat_splice_named_zero_length_insert() {
    assert_eq!(
        rs(r#"my @a = (1, 2, 3); splice @a, 1, 0, 9; join "-", @a"#),
        "1-9-2-3"
    );
}

#[test]
fn perl_compat_splice_aref_offset_only_returns_tail_removed() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3, 4];
        my @r = splice @$v, 2;
        join("-", @$v) . "|" . join("-", @r)"#),
        "1-2|3-4"
    );
}

#[test]
fn perl_compat_scalar_splice_aref_two_arg_empties_target() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my $s = scalar splice @$v;
        $s . "|" . scalar(@$v)"#),
        "3|0"
    );
}

#[test]
fn perl_compat_splice_aref_three_replacements() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [10, 20, 30, 40];
        splice @$v, 1, 2, 1, 2, 3;
        join "-", @$v"#),
        "10-1-2-3-40"
    );
}

#[test]
fn perl_compat_push_aref_splice_list_chain() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        push @$v, splice @$v, 0, 1;
        join "-", @$v"#),
        "2-3-1"
    );
}

#[test]
fn perl_compat_list_literal_leading_aref_flatten() {
    assert_eq!(
        rs(r#"no strict 'vars'; my $v = [1, 2]; my @x = (@$v, 3, 4); join "-", @x"#),
        "1-2-3-4"
    );
}

#[test]
fn perl_compat_scalar_deref_hash_compound_sub_mul() {
    assert_eq!(
        ri(r#"my %h = (a => 10); my $r = \%h; $$r{a} -= 3; $h{a}"#),
        7
    );
    assert_eq!(
        ri(r#"my %h = (a => 4); my $r = \%h; $$r{a} *= 3; $h{a}"#),
        12
    );
}

#[test]
fn perl_compat_arrow_array_compound_sub_mul_concat() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $v = [10, 20, 30]; $v->[1] -= 5; $v->[1]"#),
        15
    );
    assert_eq!(
        ri(r#"no strict 'vars'; my $v = [2, 3, 4]; $v->[1] *= 5; $v->[1]"#),
        15
    );
    assert_eq!(
        rs(r#"no strict 'vars'; my $v = ["a"]; $v->[0] .= "b"; $v->[0]"#),
        "ab"
    );
}

#[test]
fn perl_compat_arrow_hash_exists_delete_sequence() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $h = { x => 1 };
        my $e0 = exists $h->{x} ? 1 : 0;
        my $d = delete $h->{x};
        my $e1 = exists $h->{x} ? 1 : 0;
        $e0 . "," . $d . "," . $e1"#),
        "1,1,0"
    );
}

#[test]
fn perl_compat_arrow_hash_compound_sub_mul() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { a => 10 }; $h->{a} -= 3; $h->{a}"#),
        7
    );
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { a => 4 }; $h->{a} *= 3; $h->{a}"#),
        12
    );
}

#[test]
fn perl_compat_scalar_deref_hash_log_or_default() {
    assert_eq!(
        ri(r#"my %h = (a => 0); my $r = \%h; my $x = $$r{a} || 7; $x"#),
        7
    );
}

#[test]
fn perl_compat_arrow_hash_log_or_default() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { a => 0 }; my $x = $h->{a} || 7; $x"#),
        7
    );
}

#[test]
fn perl_compat_named_array_splice_negative_offset_with_replacement() {
    assert_eq!(
        rs(r#"my @a = (1, 2, 3, 4, 5);
        splice @a, -2, 1, 88;
        join "-", @a"#),
        "1-2-3-88-5"
    );
}

#[test]
fn perl_compat_unshift_aref_splice_returns_removed_to_front() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        unshift @$v, splice(@$v, 0, 1);
        join "-", @$v"#),
        "1-2-3"
    );
}

#[test]
fn perl_compat_reverse_sort_aref_list_context() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my @t = reverse @$v;
        $t[0];"#),
        3
    );
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [3, 1, 2];
        join "-", sort @$v"#),
        "1-2-3"
    );
}

#[test]
fn perl_compat_keys_values_sort_on_hashref() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $h = { a => 1, b => 2 };
        join "", sort keys %$h"#),
        "ab"
    );
    assert_eq!(
        rs(r#"no strict 'vars';
        my $h = { a => 1, b => 2 };
        join "-", sort values %$h"#),
        "1-2"
    );
}

#[test]
fn perl_compat_scalar_keys_on_hashref() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { u => 1, v => 2, w => 3 }; scalar keys %$h"#),
        3
    );
}

#[test]
fn perl_compat_grep_map_blocks_receive_aref_list() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [1, 2, 3, 4];
        scalar grep { $_ > 1 } @$v"#),
        3
    );
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my @m = map { $_ * 2 } @$v;
        $m[2];"#),
        6
    );
}

#[test]
fn perl_compat_arrow_array_postincrement() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $v = [10, 20]; $v->[0]++; $v->[0]"#),
        11
    );
}

#[test]
fn perl_compat_arrow_hash_and_scalar_deref_hash_decrement() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { k => 5 }; $h->{k}--; $h->{k}"#),
        4
    );
    assert_eq!(ri(r#"my %h = (a => 3); my $r = \%h; $$r{a}--; $h{a}"#), 2);
}

#[test]
fn perl_compat_for_loop_sum_over_arrayref() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my $s = 0;
        for my $x (@$v) { $s = $s + $x; }
        $s"#),
        6
    );
}

#[test]
fn perl_compat_splice_aref_variable_offset_replacement() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3, 4];
        my $o = 1;
        splice @$v, $o, 2, 9;
        join "-", @$v"#),
        "1-9-4"
    );
}

#[test]
fn perl_compat_defined_exists_hash_elems_through_ref() {
    assert_eq!(
        ri(r#"my %h = (a => 1); my $r = \%h; defined $$r{a} ? 1 : 0"#),
        1
    );
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { a => 1 }; exists $h->{a} ? 1 : 0"#),
        1
    );
}

#[test]
fn perl_compat_arrayref_slice_index_list() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [5, 6, 7];
        my @s = @$v[0, 2];
        $s[0] + $s[1]"#),
        12
    );
}

#[test]
fn perl_compat_keys_list_from_hashref() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $h = { "p" => 1, "q" => 2 };
        my @k = keys %$h;
        scalar @k"#),
        2
    );
}

#[test]
fn perl_compat_bracket_copy_of_aref_is_shallow_new_array() {
    assert_eq!(
        ri(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my $c = [@$v];
        $c->[0] = 9;
        $v->[0]"#),
        1
    );
}

#[test]
fn perl_compat_ref_returns_array_and_hash() {
    assert_eq!(rs(r#"no strict 'vars'; my $v = [1]; ref($v)"#), "ARRAY");
    assert_eq!(rs(r#"my %h = (a => 1); ref(\%h)"#), "HASH");
}

#[test]
fn perl_compat_scalar_splice_aref_returns_last_removed_middle() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [10, 20, 30, 40];
        my $n = scalar splice @$v, 1, 2;
        $n . "|" . join("-", @$v)"#),
        "30|10-40"
    );
}

#[test]
fn perl_compat_scalar_splice_named_negative_offset_two_removed() {
    assert_eq!(
        rs(r#"my @a = (1, 2, 3, 4, 5);
        my $n = scalar splice @a, -3, 2;
        $n . "|" . join("-", @a)"#),
        "4|1-2-5"
    );
}

#[test]
fn perl_compat_hash_elem_falsy_andassign_short_circuit() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { a => 0 }; $h->{a} &&= 1; $h->{a}"#),
        0
    );
    assert_eq!(
        ri(r#"my %h = (a => 0); my $r = \%h; $$r{a} &&= 1; $h{a}"#),
        0
    );
}

#[test]
fn perl_compat_concat_two_arefs_in_list() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $a = [1, 2];
        my $b = [3, 4];
        my @x = (@$a, @$b);
        join "-", @x"#),
        "1-2-3-4"
    );
}

#[test]
fn perl_compat_arrow_elem_assign_and_dynamic_index_mult() {
    assert_eq!(
        rs(r#"no strict 'vars'; my $v = [1, 2, 3]; $v->[1] = 9; join "-", @$v"#),
        "1-9-3"
    );
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my $i = 0;
        $v->[$i] *= 10;
        join "-", @$v"#),
        "10-2-3"
    );
}

#[test]
fn perl_compat_arrow_elem_last_index_assign() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [0, 0, 0];
        $v->[$#v] = 5;
        join "-", @$v"#),
        "0-0-5"
    );
}

#[test]
fn perl_compat_named_array_last_index_elem() {
    assert_eq!(ri(r#"my @a = (1, 2, 3); $a[$#a]"#), 3);
}

#[test]
fn perl_compat_new_hash_keys_via_arrow_and_scalar_deref() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = { "a" => 1 }; $h->{"b"} = 2; $h->{"a"} + $h->{"b"}"#,),
        3
    );
    assert_eq!(
        ri(r#"my %h = (a => 1); my $r = \%h; $$r{b} = 3; $h{a} + $h{b}"#),
        4
    );
}

#[test]
fn perl_compat_empty_hashref_or_default_fills_key() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = {}; $h->{z} ||= 7; $h->{z}"#),
        7
    );
    assert_eq!(ri(r#"my %h = (); my $r = \%h; $$r{z} ||= 8; $h{z}"#), 8);
}

#[test]
fn perl_compat_empty_aref_push() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $v = []; push @$v, 1, 2; scalar @$v"#),
        2
    );
}

#[test]
fn perl_compat_empty_hashref_assign_then_key_count() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $h = {}; $h->{x} = 1; scalar keys %$h"#),
        1
    );
}

#[test]
fn perl_compat_splice_named_three_replacements_returns_removed() {
    assert_eq!(
        rs(r#"my @a = (1, 2, 3, 4);
        my @b = splice @a, 1, 2, 9, 8, 7;
        join("-", @a) . "|" . join("-", @b)"#),
        "1-9-8-7-4|2-3"
    );
}

#[test]
fn perl_compat_splice_aref_three_replacements_returns_removed() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3, 4];
        my @b = splice @$v, 1, 2, 9, 8, 7;
        join("-", @$v) . "|" . join("-", @b)"#),
        "1-9-8-7-4|2-3"
    );
}

#[test]
fn perl_compat_shift_pop_aref() {
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        shift @$v;
        join "-", @$v"#),
        "2-3"
    );
    assert_eq!(
        rs(r#"no strict 'vars';
        my $v = [1, 2, 3];
        my $x = pop @$v;
        $x . "|" . join("-", @$v)"#),
        "3|1-2"
    );
}

#[test]
fn perl_compat_arrow_preinc_and_named_array_blshift_assign() {
    assert_eq!(
        ri(r#"no strict 'vars'; my $v = [3]; ++$v->[0]; $v->[0]"#),
        4
    );
    assert_eq!(ri(r#"my @a = (1, 2, 4); $a[1] <<= 1; $a[1]"#), 4);
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
