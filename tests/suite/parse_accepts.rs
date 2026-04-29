//! Parser accepts valid Perl fragments (explicit `#[test]` per case; no macro batching).

fn p(src: &str) {
    stryke::parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"));
}

#[test]
fn accepts_ampersand_qualified_subroutine_ref() {
    p("&foo::bar");
}

#[test]
fn accepts_integer_literal_statement() {
    p("42");
}

#[test]
fn accepts_negative_integer() {
    p("-99");
}

#[test]
fn accepts_float_literal() {
    p("0.25");
}

#[test]
fn accepts_scientific_float() {
    p("1.5e2");
}

#[test]
fn accepts_hex_literal() {
    p("0xdeadbeef");
}

#[test]
fn accepts_binary_literal() {
    p("0b1010");
}

#[test]
fn accepts_octal_literal() {
    p("0777");
}

#[test]
fn accepts_double_quoted_string() {
    p(r#""hello\n""#);
}

#[test]
fn accepts_single_quoted_string() {
    p("'world'");
}

#[test]
fn accepts_qq_constructor() {
    p(r#"qq(embedded)"#);
}

#[test]
fn accepts_q_constructor() {
    p(r#"q{braces}"#);
}

#[test]
fn accepts_qw_word_list() {
    p("my @w = qw(alpha beta gamma)");
}

#[test]
fn accepts_my_scalar() {
    p("my $n");
}

#[test]
fn accepts_my_scalar_init() {
    p("my $n = 1");
}

#[test]
fn accepts_my_array() {
    p("my @a");
}

#[test]
fn accepts_my_hash() {
    p("my %h");
}

#[test]
fn accepts_our_scalar() {
    p("our $g");
}

#[test]
fn accepts_local_scalar() {
    p("local $l");
}

#[test]
fn accepts_scalar_assign() {
    p("$x = 3");
}

#[test]
fn accepts_array_assign_list() {
    p("@a = (1, 2, 3)");
}

#[test]
fn accepts_hash_assign_fat_comma() {
    p("%h = (k => 1)");
}

#[test]
fn accepts_addition() {
    p("3 + 4");
}

#[test]
fn accepts_subtraction() {
    p("10 - 3");
}

#[test]
fn accepts_multiplication() {
    p("6 * 7");
}

#[test]
fn accepts_division() {
    p("15 / 3");
}

#[test]
fn accepts_modulo() {
    p("17 % 5");
}

#[test]
fn accepts_power_right_assoc() {
    p("2 ** 3 ** 2");
}

#[test]
fn accepts_numeric_equality() {
    p("5 == 5");
}

#[test]
fn accepts_numeric_inequality() {
    p("5 != 3");
}

#[test]
fn accepts_spaceship() {
    p("3 <=> 5");
}

#[test]
fn accepts_string_cmp() {
    p(r#""a" cmp "b""#);
}

#[test]
fn accepts_string_eq_ne() {
    p(r#""a" eq "a""#);
    p(r#""a" ne "b""#);
}

#[test]
fn accepts_string_lt_gt() {
    p(r#""a" lt "b""#);
    p(r#""b" gt "a""#);
}

#[test]
fn accepts_string_le_ge() {
    p(r#""a" le "b""#);
    p(r#""b" ge "a""#);
}

#[test]
fn accepts_logical_and_short_circuit() {
    p("1 && 2");
}

#[test]
fn accepts_logical_or_short_circuit() {
    p("0 || 7");
}

#[test]
fn accepts_defined_or() {
    p("undef // 5");
}

#[test]
fn accepts_lowercase_and_or_not() {
    p("1 and 2");
    p("0 or 9");
    p("not 0");
}

#[test]
fn accepts_bitwise_ops() {
    p("0x0f & 0x33");
    p("0x10 | 0x01");
    p("0b1010 ^ 0b1100");
}

#[test]
fn accepts_shifts() {
    p("32 >> 3");
    p("1 << 4");
}

#[test]
fn accepts_unary_bitwise_not() {
    p("~0");
}

#[test]
fn accepts_string_concat() {
    p(r#""a" . "b""#);
}

#[test]
fn accepts_string_repeat() {
    p(r#""x" x 3"#);
}

#[test]
fn accepts_ternary() {
    p("1 ? 2 : 3");
}

#[test]
fn accepts_if_block() {
    p("if (1) { 1; }");
}

#[test]
fn accepts_unless_block() {
    p("unless (0) { 1; }");
}

#[test]
fn accepts_if_elsif_else() {
    p("if (0) { 1; } elsif (1) { 2; } else { 3; }");
}

#[test]
fn accepts_while_loop() {
    p("while (0) { }");
}

#[test]
fn accepts_until_loop() {
    p("until (1) { }");
}

#[test]
fn accepts_c_style_for() {
    p("for (my $i = 0; $i < 3; $i = $i + 1) { 1; }");
}

#[test]
fn accepts_foreach_my() {
    p("foreach my $v (1, 2) { $v; }");
}

#[test]
fn accepts_foreach_implicit_topic() {
    p("foreach (1, 2) { $_; }");
}

#[test]
fn accepts_do_while() {
    p("do { 1 } while (0)");
}

#[test]
fn accepts_do_block() {
    p("do { 1 }");
}

#[test]
fn accepts_postfix_modifier_if() {
    p("$x = 1 if 1");
}

#[test]
fn accepts_range_dots() {
    p("1..10");
}

#[test]
fn accepts_range_three_dot() {
    p("1...10");
    p("my @x = (1...3)");
}

#[test]
fn accepts_list_paren() {
    p("(1, 2, 3)");
}

#[test]
fn accepts_array_index() {
    p("$a[0]");
}

#[test]
fn accepts_hash_slice_brace() {
    p("$h{key}");
    p("@h{'a', 'b'} = (1, 2)");
}

#[test]
fn accepts_keys_values_each() {
    p("keys %h");
    p("values %h");
    p("each %h");
}

#[test]
fn accepts_delete_exists() {
    p("delete $h{k}");
    p("exists $h{k}");
}

#[test]
fn accepts_push_pop_shift_unshift_splice() {
    p("push @a, 1");
    p("pop @a");
    p("shift @a");
    p("unshift @a, 1");
    p("splice @a, 0, 1");
}

#[test]
fn accepts_scalar_aggregate() {
    p("len(@a)");
    p("scalar %h");
}

#[test]
fn accepts_m_regex() {
    p("m/foo/");
    p("m#bar#");
}

#[test]
fn accepts_s_substitute() {
    p("s/a/b/");
}

#[test]
fn accepts_qr() {
    p("qr/x/");
    p("qr{y}i");
}

#[test]
fn accepts_transliterate() {
    p("y/a/b/");
    p("tr/a/b/");
}

#[test]
fn accepts_grep_map_sort() {
    p("grep { $_ } (1)");
    p("map { $_ } (1)");
    p("sort (1,2)");
}

#[test]
fn accepts_defined_undef_ref() {
    p("defined $x");
    p("undef $x");
    p("ref $x");
}

#[test]
fn accepts_bless_caller() {
    p("bless {}, 'C'");
    p("caller()");
}

#[test]
fn accepts_eval_forms() {
    p("eval '1'");
    p("eval { 1 }");
}

#[test]
fn accepts_require_use_no_package() {
    p("require strict");
    p("use strict");
    p("no warnings");
    p("package P::Q");
}

#[test]
fn accepts_do_string() {
    p(r#"do "foo.pl""#);
}

#[test]
fn accepts_sub_decl() {
    p("fn foo { }");
    p("fn g ($$) { return $_0 + $_1; }");
}

#[test]
fn accepts_return_loop_control() {
    p("return");
    p("return 1");
    p("last");
    p("next");
    p("redo");
}

#[test]
fn accepts_begin_end() {
    p("BEGIN { 1; }");
    p("END { 1; }");
}

#[test]
fn accepts_chomp_chop_length() {
    p("chomp $x");
    p("chop $x");
    p("length $x");
}

#[test]
fn accepts_substr_index_rindex() {
    p("substr $x, 0, 1");
    p("index 'a','a'");
    p("rindex 'a','a'");
}

#[test]
fn accepts_sprintf_printf_print_say() {
    p("sprintf '%d', 1");
    p("printf '%d', 1");
    p("print 1");
    p("p 1");
}

#[test]
fn accepts_case_folding() {
    p("lc 'A'");
    p("uc 'a'");
    p("lcfirst 'A'");
    p("ucfirst 'a'");
}

#[test]
fn accepts_hex_oct_int_abs_sqrt_chr_ord() {
    p("hex '10'");
    p("oct '10'");
    p("int 1.2");
    p("abs -1");
    p("sqrt 4");
    p("chr 65");
    p("ord 'A'");
}

#[test]
fn accepts_join_split_reverse_sort_block() {
    p("join ',', (1,2)");
    p("split /,/, 'a,b'");
    p("rev (1,2)");
    p("reversed (1,2)");
    p("sort { $a <=> $b } (2,1)");
}

#[test]
fn accepts_open_close_eof_tell() {
    p("open F, '<', 'x'");
    p("close F");
    p("eof");
    p("tell F");
}

#[test]
fn accepts_mkdir_rmdir_unlink_chdir() {
    p("mkdir 'x'");
    p("rmdir 'x'");
    p("unlink 'x'");
    p("chdir '.'");
}

#[test]
fn accepts_system_exec_exit() {
    p("system 'true'");
    p("exec 'true'");
    p("exit");
    p("exit 0");
}

#[test]
fn accepts_time_rand_trig() {
    p("time()");
    p("rand()");
    p("srand()");
    p("cos(0)");
    p("sin(0)");
    p("exp(0)");
    p("log(1)");
}

#[test]
fn accepts_vec_pack_unpack() {
    p("vec($v, 0, 8)");
    p("pack 'C*', 65, 66");
    p("unpack 'C', 'A'");
}

#[test]
fn accepts_continue() {
    p("continue { }");
}

#[test]
fn accepts_tie_select_binmode() {
    p("tie %h, 'Tie::Std'");
    p("untie $x");
    p("select STDOUT");
    p("select STDERR");
    p("binmode STDOUT");
}

#[test]
fn accepts_file_ops_rename_stat() {
    p("truncate F, 0");
    p("rename 'a', 'b'");
    p("link 'a', 'b'");
    p("symlink 'a', 'b'");
    p("readlink 'x'");
    p("stat 'x'");
    p("lstat 'x'");
    p("utime 1, 2, 'f'");
    p("chmod 0755, 'f'");
    p("chown 0, 0, 'f'");
}

#[test]
fn accepts_dir_ops() {
    p("opendir D, '.'");
    p("readdir D");
    p("rewinddir D");
    p("closedir D");
    p("telldir D");
    p("seekdir D, 0");
}

#[test]
fn accepts_alarm_sleep_process() {
    p("alarm 1");
    p("sleep 0");
    p("getppid()");
    p("getpgrp()");
    p("setpgrp()");
    p("getpriority 0, 0");
    p("setpriority 0, 0, 0");
    p("times()");
}

#[test]
fn accepts_time_local_gmtime() {
    p("localtime()");
    p("gmtime()");
    p("time()");
    p("getlogin()");
}

#[test]
fn accepts_user_group_lookups() {
    p("getpwuid 0");
    p("getpwnam 'root'");
    p("getgrgid 0");
    p("getgrnam 'wheel'");
}

#[test]
fn accepts_network_lookups() {
    p("gethostbyname 'localhost'");
    p("getprotobyname 'tcp'");
    p("getservbyname 'http', 'tcp'");
}

#[test]
fn accepts_socket_primitives() {
    p("socket S, 2, 1, 0");
    p("bind S, $addr");
    p("listen S, 5");
    p("accept NS, S");
    p("connect S, $addr");
    p("shutdown S, 2");
    p("setsockopt S, 1, 1, 1");
    p("getsockopt S, 1, 1");
    p("getpeername S");
    p("getsockname S");
    p("send S, 'x', 0");
    p("recv S, $buf, 100, 0");
}

#[test]
fn accepts_fork_wait_pipe_open2() {
    p("fork()");
    p("wait()");
    p("waitpid -1, 0");
    p("pipe R, W");
    p("open2 R, W, 'true'");
}

#[test]
fn accepts_qx_backtick_readline_getc_read() {
    p("qx(true)");
    p("`true`");
    p("readline F");
    p("getc F");
    p("read F, $buf, 10");
    p("sysread F, $buf, 10, 0");
}

#[test]
fn accepts_compound_assignments_in_statement() {
    p("my $x = 10; $x += 3");
    p("my $x = 10; $x -= 4");
    p("my $x = 2; $x *= 3");
    p("my $x = 2; $x **= 3");
    p("my $x = 10; $x %= 3");
    p(r#"my $s = "a"; $s .= "b""#);
}

#[test]
fn accepts_increment_decrement() {
    p("my $x = 1; ++$x");
    p("my $x = 1; $x++");
    p("my $x = 3; --$x");
    p("my $x = 3; $x--");
}

#[test]
fn accepts_postfix_loop_modifiers() {
    p("$sum = $sum + $_ for 1,2,3");
    p("$x++ while $x < 4");
    p("$x++ until $x >= 4");
}

#[test]
fn accepts_postfix_for_on_do_bare_block_and_parallel() {
    p(r#"do { print "x" } for @a"#);
    p(r#"{ print "x" } for @a"#);
    p("pmap { $_ } @x for @y");
    p("pfor { 1 } @x for @y");
    p("pgrep { $_ } @x for @y");
    p("pmap_chunked 2 { $_ } @x, @y for @z");
}

#[test]
fn accepts_for_last_next() {
    p("for my $i (1..10) { last if $i > 5; }");
    p("for my $i (1..10) { next if $i % 2 == 0; }");
}

#[test]
fn accepts_wantarray() {
    p("wantarray");
}

#[test]
fn accepts_join_puniq_call() {
    p(r#"(1, 2, 2, 1, 3, 1) |> puniq |> join ','"#);
    p(r#"(1, 2, 2, 1, 3, 1) |> puniq |> join ','"#);
}

#[test]
fn accepts_bare_uniq_any_all_none() {
    p(r#"(1, [2, 3]) |> flatten |> join ','"#);
    p(r#"list_count(1, 2, 3)"#);
    p(r#"list_size()"#);
    p(r#"(1, 1, 2, 3) |> distinct |> join ','"#);
    p(r#"(1, 1, 2, 3) |> uniq |> join ','"#);
    p(r#"(1, 2, 3) |> shuffle |> join '-'"#);
    p(r#"scalar (1, 2, 3, 4) |> chunked 2"#);
    p(r#"scalar (1, 2, 3) |> windowed 2"#);
    p(r#"windowed(2)"#);
    p(r#"chunked(2)"#);
    p(r#"(1, 2) |> chunked 2"#);
    p(r#"(9, 8) |> windowed 2"#);
    p(r#"(1, 2) |> fold { $a + $b }"#);
    p(r#"(1, 2, 3) |> fold { $a + $b }"#);
    p(r#"qw(x y) |> fold { $a . $b }"#);
    p(r#"(1, 2) |> take_while { $_ < 9 } |> join ','"#);
    p(r#"(1, 2) |> drop_while { 0 } |> join ','"#);
    p(r#"(1, 2) |> tap { 1 } |> join ','"#);
    p(r#"(1, 2) |> peek { 1 } |> join ','"#);
    p(r#"list_count(with_index((1, 2)))"#);
    p(r#"(1, 2, 3) |> with_index"#);
    p(r#"any { $_ > 1 } (1, 2, 3)"#);
    p(r#"all { $_ > 0 } (1, 2)"#);
    p(r#"none { $_ < 0 } (1, 2)"#);
    p(r#"inject { $a + $b } (1, 2, 3)"#);
    p(r#"detect { $_ > 1 } (1, 2, 3)"#);
    p(r#"find { $_ > 1 } (1, 2, 3)"#);
    p(r#"find_all { $_ % 2 == 0 } (1, 2, 3, 4)"#);
    p(r#"chunk_by { $_ % 2 } (1, 3, 2)"#);
    p(r#"group_by { 0 } (9)"#);
    p(r#"group_by $_ % 2, (1, 2)"#);
    p(r#"(1, 2, 3) |> chunk_by { $_ }"#);
}
