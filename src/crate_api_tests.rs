//! Unit tests for the crate root API: `parse`, `run`, `parse_and_run_string`, `try_vm_execute`.

use crate::interpreter::Interpreter;
use crate::{lint_program, parse, parse_and_run_string, parse_with_file, run, try_vm_execute};

fn run_int(code: &str) -> i64 {
    run(code).expect("run").to_int()
}

#[test]
fn run_arithmetic_add_sub_mul_div_mod() {
    assert_eq!(run_int("11 + 4"), 15);
    assert_eq!(run_int("20 - 7"), 13);
    assert_eq!(run_int("6 * 9"), 54);
    assert_eq!(run_int("22 / 4"), 5);
    assert_eq!(run_int("17 % 5"), 2);
}

#[test]
fn run_power_and_precedence() {
    assert_eq!(run_int("2 ** 8"), 256);
    assert_eq!(run_int("2 + 3 * 4"), 14);
    assert_eq!(run_int("(2 + 3) * 4"), 20);
}

#[test]
fn run_compound_assign_xor_shift() {
    assert_eq!(run_int(r#"my $x = 4; $x <<= 2; $x"#), 16);
    assert_eq!(run_int(r#"my $x = 16; $x >>= 2; $x"#), 4);
    assert_eq!(run_int(r#"my $x = 5; $x ^= 3; $x"#), 6);
}

#[test]
fn run_numeric_comparisons_yield_perl_truth() {
    assert_eq!(run_int("5 == 5"), 1);
    assert_eq!(run_int("5 != 3"), 1);
    assert_eq!(run_int("3 < 5"), 1);
    assert_eq!(run_int("5 > 3"), 1);
    assert_eq!(run_int("5 <= 5"), 1);
    assert_eq!(run_int("5 >= 4"), 1);
}

#[test]
fn run_spaceship_operator() {
    assert_eq!(run_int("5 <=> 3"), 1);
    assert_eq!(run_int("3 <=> 5"), -1);
    assert_eq!(run_int("4 <=> 4"), 0);
}

#[test]
fn run_string_cmp_and_eq() {
    assert_eq!(run_int(r#""a" cmp "b""#), -1);
    assert_eq!(run_int(r#""b" cmp "a""#), 1);
    assert_eq!(run_int(r#""a" eq "a""#), 1);
    assert_eq!(run_int(r#""a" ne "b""#), 1);
}

#[test]
fn run_logical_short_circuit() {
    assert_eq!(run_int("1 && 7"), 7);
    assert_eq!(run_int("0 && 7"), 0);
    assert_eq!(run_int("0 || 8"), 8);
    assert_eq!(run_int("3 || 8"), 3);
}

#[test]
fn run_log_and_compound_assign() {
    assert_eq!(run_int("my $x = 0; $x &&= 5; $x"), 0);
    assert_eq!(run_int("my $y = 2; $y &&= 7; $y"), 7);
}

#[test]
fn run_defined_or_operator() {
    assert_eq!(run_int("undef // 99"), 99);
    assert_eq!(run_int("0 // 5"), 0);
}

#[test]
fn run_bitwise_ops() {
    assert_eq!(run_int("0x0F & 0x33"), 0x03);
    assert_eq!(run_int("0x01 | 0x02"), 0x03);
    assert_eq!(run_int("0x0F ^ 0x33"), 0x3C);
}

#[test]
fn run_unary_minus_and_not() {
    assert_eq!(run_int("- 42"), -42);
    assert_eq!(run_int("!0"), 1);
    assert_eq!(run_int("!1"), 0);
}

#[test]
fn run_concat_and_repeat() {
    assert_eq!(run(r#""a" . "b" . "c""#).expect("run").to_string(), "abc");
    assert_eq!(run(r#""x" x 4"#).expect("run").to_string(), "xxxx");
}

#[test]
fn run_list_and_scalar_context_array() {
    assert_eq!(run_int("scalar (1, 2, 3)"), 3);
}

#[test]
fn run_my_variable_and_assignment() {
    assert_eq!(run_int("my $x = 41; $x + 1"), 42);
}

#[test]
fn run_conditional_expression() {
    assert_eq!(run_int("1 ? 10 : 20"), 10);
    assert_eq!(run_int("0 ? 10 : 20"), 20);
}

#[test]
fn run_simple_subroutine() {
    assert_eq!(run_int("sub add2 { return $_0 + $_1; } add2(30, 12)"), 42);
}

#[test]
fn parse_with_file_includes_path_in_syntax_error_display() {
    let e = parse_with_file("sub f {", "/tmp/parity_syntax_path.pm").expect_err("unclosed brace");
    let s = e.to_string();
    assert!(
        s.contains("/tmp/parity_syntax_path.pm"),
        "expected path in error, got: {s}"
    );
}

#[test]
fn parse_and_run_string_shares_interpreter_state() {
    let mut i = Interpreter::new();
    parse_and_run_string("my $crate_api_z = 100;", &mut i).expect("first");
    let v = parse_and_run_string("$crate_api_z + 1;", &mut i).expect("second");
    assert_eq!(v.to_int(), 101);
}

#[test]
fn try_vm_execute_runs_simple_literal_program() {
    let p = parse("42").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some());
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

/// `pos = EXPR` updates `regex_pos` for `$_` (Text::Balanced / `m//gc` preamble).
#[test]
fn run_pos_assign_implicit_underbar_reads_back() {
    assert_eq!(run_int(r#"$_ = "zz"; pos = 2; pos"#), 2);
}

#[test]
fn run_pos_assign_named_scalar_reads_back() {
    assert_eq!(run_int(r#"my $s = "ab"; pos $s = 1; pos $s"#), 1);
}

#[test]
fn try_vm_execute_pos_assign_sets_regex_pos() {
    let p = parse(r#"$_ = ""; pos = 3; pos"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "pos = should compile on VM (SetRegexPos) and return pos"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

/// `pos $$r = …` (Text::Balanced `_match_bracketed`).
#[test]
fn run_pos_deref_scalar_assign_reads_back() {
    assert_eq!(
        run_int(
            r#"no strict 'vars'
            my $s = "ab"
            my $r = \$s
            pos $$r = 1
            pos $$r"#
        ),
        1
    );
}

#[test]
fn try_vm_execute_pos_deref_scalar_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $s = ""
        my $t = \$s
        pos $$t = 2
        pos $$t"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        r"pos $$r = should compile (SetRegexPos + deref key)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
}

/// `map EXPR, LIST` with a builtin call (not only arithmetic).
#[test]
fn try_vm_execute_map_expr_comma_length_builtin() {
    let p = parse(
        r#"no strict 'vars'
        join(",", map length, qw(a bb))"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "map length, LIST should stay on VM (Op::MapWithExpr)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,2");
}

/// `tell` after `print` shares the file cursor (`Interpreter` I/O slot).
#[test]
fn try_vm_execute_tell_after_print_to_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_tell_api_test");
    let _ = std::fs::remove_file(&path);
    let ps = path.to_string_lossy();
    let src = format!(r#"open F, ">", "{ps}"; print F "xyzzy"; my $p = tell F; close F; $p"#);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "tell after print should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 5);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_quotemeta_builtin() {
    let p = parse(r#"quotemeta("a.c")"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "quotemeta should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), r"a\.c");
}

/// Last stmt in `do { }` must see list context when assigning to an array (grep uniq idiom).
#[test]
fn try_vm_execute_do_block_propagates_list_context_to_grep() {
    let p = parse(
        r#"my @l = (1, 2, 3, 2, 1)
        my @u = do { my %seen; grep { !$seen{$_}++ } @l }
        scalar @u"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "do block with grep in list assign should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

/// `chomp` on a lexical scalar — [`Op::ChompInPlace`] + return value (tree uses same helper).
#[test]
fn try_vm_execute_chomp_scalar_returns_removed_count() {
    let p = parse(
        r#"my $s = "xy\n"
        my $n = chomp $s
        $n * 100 + length($s)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "chomp should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 102);
}

/// `s///` pattern expands `$ENV{KEY}` (zpwr-style paths) on the VM.
#[test]
fn try_vm_execute_subst_pattern_expands_env_brace() {
    let home = std::env::var("HOME").expect("HOME");
    let src = format!(r#"$_ = "{home}/tail"; s@$ENV{{HOME}}@~@; $_;"#);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "s/// with $ENV in pattern should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "~/tail");
}

/// `s///` replacement expands `$ENV{KEY}` on the VM (capture groups + env).
#[test]
fn try_vm_execute_subst_replacement_expands_env_brace() {
    let home = std::env::var("HOME").expect("HOME");
    let p = parse(
        r#"$_ = "~/baz"
        s@^([~])([^~]*)$@$ENV{HOME}$2@
        $_"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "s/// replacement with $ENV should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), format!("{home}/baz"));
}

/// `my @a = do { … }` — RHS of array assign must use list context inside `do`.
#[test]
fn try_vm_execute_my_array_assign_do_block_list_rhs() {
    let p = parse(
        r#"my @a = do { (7, 8, 9) }
        $a[0] * 100 + $a[1] * 10 + $a[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "my @a = do-block with list RHS should compile on VM (list wantarray in do)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 789);
}

/// `my %h = do { … }` — hash assign RHS is list context (pairs from the list).
#[test]
fn try_vm_execute_my_hash_assign_do_block_list_rhs() {
    let p = parse(
        r#"my %h = do { ("a", 2, "b", 5) }
        $h{"a"} * 10 + $h{"b"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "my %h = do-block with list RHS should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 25);
}

#[test]
fn try_vm_execute_tell_stdout_returns_negative_one() {
    let p = parse("tell STDOUT").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "tell STDOUT should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), -1);
}

#[test]
fn try_vm_execute_core_tell_stdout() {
    let p = parse("CORE::tell STDOUT").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "CORE::tell should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), -1);
}

/// `@a = %h` flattens key/value pairs (list context on `%h`).
#[test]
fn try_vm_execute_array_assign_flattens_hash() {
    let p = parse(
        r#"my %h = ("u", 1, "v", 2)
        my @a = %h
        scalar @a"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "assign array from hash as list should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 4);
}

#[cfg(unix)]
#[test]
fn try_vm_execute_fileno_stdout() {
    let p = parse("fileno STDOUT").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "fileno should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

#[test]
fn try_vm_execute_getc_reads_from_open_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_getc_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"QZ").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(
        r#"open F, "<", "{ps}";
        my $s = (getc F) . (getc F);
        close F;
        $s;"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "getc on file should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "QZ");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_binmode_stdout() {
    let p = parse("binmode STDOUT").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "binmode should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

/// `$"` drives `join` glue and [`Interpreter::list_separator`] (array interpolation uses same store).
#[test]
fn try_vm_execute_join_uses_list_separator_glue() {
    let p = parse(
        r#"no strict 'vars'
        $" = "-"
        join $", ("aa", "bb", "cc")"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "join with glue from list separator should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "aa-bb-cc");
}

#[test]
fn try_vm_execute_qq_array_respects_custom_list_separator() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 2, 3)
        $" = "|"
        "@a""#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "qq array interpolation should use updated list separator on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1|2|3");
}

#[test]
fn try_vm_execute_scalar_keys_hash_count() {
    let p = parse(
        r#"my %h = ("a", 1, "b", 2, "c", 3)
        scalar keys %h"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar keys on hash should compile on VM (HashKeysScalar)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_scalar_values_hash_count() {
    let p = parse(
        r#"my %h = ("a", 1, "b", 2)
        scalar values %h"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar values on hash should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
}

#[test]
fn try_vm_execute_join_reverse_list() {
    let p = parse(r#"join(",", reverse (30, 20, 10))"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "reverse list then join should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,20,30");
}

#[test]
fn try_vm_execute_sysseek_seek_set_then_tell() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_sysseek_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"WXYZ").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(
        r#"open H, "<", "{ps}";
        sysseek H, 2, 0;
        my $p = tell H;
        close H;
        $p;"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "sysseek then tell should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_eof_false_while_input_handle_open() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_eof_open_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"x").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(
        r#"open E, "<", "{ps}";
        my $n = eof("E");
        close E;
        $n;"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "eof on open handle should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_eof_true_after_input_handle_closed() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_eof_closed_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"y").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(
        r#"open E, "<", "{ps}";
        close E;
        eof("E");"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "eof after close should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_truncate_path_shortens_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_truncate_test");
    let _ = std::fs::remove_file(&path);
    let ps = path.to_string_lossy();
    let src = format!(
        r#"open W, ">", "{ps}";
        print W "hello";
        close W;
        my $ok = truncate "{ps}", 2;
        open R, "<", "{ps}";
        my $s = readline R;
        close R;
        $ok * 100 + length $s;"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "truncate path should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 102);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_split_with_limit() {
    let p = parse(r#"scalar split(",", "aa,bb,cc,dd", 2)"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "split with LIMIT should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
}

#[test]
fn try_vm_execute_pack_unsigned_char() {
    let p = parse(r#"ord substr(pack("C", 77), 0, 1)"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "pack C should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 77);
}

#[test]
fn try_vm_execute_unpack_after_pack_unsigned_char() {
    let p = parse(r#"scalar unpack("C", pack("C", 91))"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "unpack after pack should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 91);
}

#[test]
fn try_vm_execute_eval_string_expression() {
    let p = parse(r#"eval '31 + 11'"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "eval string should compile on VM (BuiltinId::Eval)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_eval_block_expression() {
    let p = parse(r#"eval { 50 - 8 }"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "eval block should compile on VM (EvalBlock)");
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_filetest_s_nonempty_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_filetest_s");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"hi").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(r#"-s "{ps}";"#);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "-s on nonempty file should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2); // -s returns file size (2 bytes for "hi")
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_filetest_z_empty_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_filetest_z");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(r#"-z "{ps}";"#);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "-z on empty file should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_sleep_zero() {
    let p = parse("sleep 0").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "sleep should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_glob_lists_matching_files_in_dir() {
    let base = std::env::temp_dir().join(format!("stryke_vm_glob_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir temp glob");
    std::fs::write(base.join("a.txt"), b"x").expect("write a.txt");
    std::fs::write(base.join("b.txt"), b"y").expect("write b.txt");
    std::fs::write(base.join("n.md"), b"z").expect("write n.md");
    let d = base.to_string_lossy();
    let src = format!(
        r#"my $dir = "{d}";
        my @g = glob("$dir/*.txt");
        scalar @g;"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "glob should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn try_vm_execute_qx_scalar_reads_stdout() {
    let p = parse(r#"scalar `printf '%s' vm_qx_ok`"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "qx / readpipe should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "vm_qx_ok");
}

#[test]
fn try_vm_execute_prototype_coderef() {
    let p = parse(
        r#"sub demo ($) { $_0 * 2 }
        prototype \&demo"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "prototype on coderef should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "$");
}

#[test]
fn try_vm_execute_study_non_empty_string() {
    let p = parse(r#"study "pq""#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "study should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

#[test]
fn try_vm_execute_hex_and_oct_literals() {
    let p = parse(r#"hex("2a") + oct("10")"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "hex and oct should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 42 + 8);
}

#[test]
fn try_vm_execute_select_default_output_handle_roundtrip() {
    let p = parse(
        r#"my $was = select(STDERR)
        my $prev = select($was)
        $was . ":" . $prev"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "one-arg select should compile on VM (default print handle)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "STDOUT:STDERR");
}

#[test]
fn try_vm_execute_abs_int_sqrt_builtins() {
    let p = parse(r#"abs(-11) + int(3.9) + sqrt(36)"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "abs, int, sqrt should compile on VM (CallBuiltin numeric)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 11 + 3 + 6);
}

#[test]
fn try_vm_execute_defined_builtin() {
    let p = parse(r#"defined("ok") * 100 + defined(undef)"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "defined should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 100);
}

#[test]
fn try_vm_execute_ref_scalar_reference() {
    let p = parse(
        r#"no strict 'vars'
        my $q = 0
        ref \$q"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "ref should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "SCALAR");
}

#[test]
fn try_vm_execute_bless_sets_ref_package() {
    let p = parse(
        r#"no strict 'vars'
        my $o = bless {}, "Zoo"
        ref $o"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "bless and ref should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "Zoo");
}

#[test]
fn try_vm_execute_delete_hash_key_and_exists() {
    let p = parse(
        r#"my %h = ("k", 33)
        my $d = delete $h{"k"}
        my $still = exists $h{"k"}
        $d * 10 + $still"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "delete and exists on hash should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 330);
}

#[test]
fn try_vm_execute_sin_cos_atan2_log_exp() {
    let p = parse(r#"int(100 * atan2(1, 1)) + int(sin(0) + cos(0)) + int(log(exp(4)))"#)
        .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "sin, cos, atan2, log, exp should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 78 + 1 + 4);
}

#[test]
fn try_vm_execute_index_rindex_substr() {
    let p =
        parse(r#"index("abca", "a") + 10 * rindex("abca", "a") + length substr("abcdef", 1, 3)"#)
            .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "index, rindex, substr should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 33);
}

#[test]
fn try_vm_execute_splice_returns_removed_slice() {
    let p = parse(
        r#"my @v = (10, 20, 30, 40)
        join("-", splice @v, 1, 2)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "splice should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "20-30");
}

#[test]
fn try_vm_execute_unshift_prepends() {
    let p = parse(
        r#"my @w = (9)
        unshift @w, 8
        $w[0] * 10 + $w[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "unshift should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 89);
}

#[test]
fn try_vm_execute_fc_foldcase() {
    let p = parse(r#"fc("AbC")"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "fc should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "abc");
}

#[test]
fn try_vm_execute_slurp_reads_whole_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_slurp_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"slurp-me").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(r#"length(slurp "{ps}");"#);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "slurp should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 8);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_stat_file_size_at_index_seven() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_stat_size_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"1234567").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(
        r#"my @st = stat("{ps}");
        $st[7];"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "stat should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 7);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_stat_missing_path_empty_list() {
    let p = parse(r#"my @st = stat("stryke___stat___no_such___file"); scalar @st"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "stat missing path should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_readline_scalar_first_line() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_vm_readline_scalar_{}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"first\nsecond\n").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(
        r#"open RL, "<", "{ps}";
        my $ln = readline RL;
        close RL;
        length $ln;"#
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "readline scalar should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_mkdir_and_d_filetest() {
    let base = std::env::temp_dir().join(format!("stryke_vm_mkdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let pb = base.to_string_lossy();
    let src = format!(
        r#"mkdir "{pb}";
        (-d "{pb}") * 10 + (-e "{pb}");"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "mkdir should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 11);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn try_vm_execute_capture_true_reports_zero_exit() {
    let p = parse(r#"my $r = capture("true"); $r->exitcode"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "capture should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_filetest_e_and_f_nonempty_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_ef_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"!").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(r#"((-e "{ps}") * 10) + (-f "{ps}");"#,);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "-e and -f should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 11);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn try_vm_execute_opendir_readdir_finds_known_file() {
    let base = std::env::temp_dir().join(format!("stryke_vm_od_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    std::fs::write(base.join("needle.txt"), b"ok").expect("write file");
    let pd = base.to_string_lossy();
    let src = format!(
        r#"opendir DH, "{pd}" or die;
        my @ents;
        for (1..16) {{
          my $f = readdir DH;
          last unless defined $f;
          push @ents, $f;
        }}
        closedir DH;
        scalar grep {{ $_ eq "needle.txt" }} @ents;"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "opendir/readdir/closedir should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn try_vm_execute_rewinddir_resets_telldir() {
    let base = std::env::temp_dir().join(format!("stryke_vm_rewind_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    std::fs::write(base.join("x.txt"), b"1").expect("write");
    let pd = base.to_string_lossy();
    let src = format!(
        r#"opendir D, "{pd}" or die;
        readdir D;
        rewinddir D;
        my $z = (telldir D) == 0 ? 1 : 0;
        closedir D;
        $z;"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "rewinddir/telldir should compile on VM (directory ops)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn try_vm_execute_readlink_returns_symlink_target() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("stryke_vm_rl_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    let link = base.join("L");
    symlink("rel_tg", &link).expect("symlink");
    let sl = link.to_string_lossy();
    let src = format!(r#"(readlink "{sl}") eq "rel_tg" ? 1 : 0;"#,);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "readlink should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn try_vm_execute_hard_link_shares_file_contents() {
    let dir = std::env::temp_dir();
    let a = dir.join(format!("stryke_vm_hl_a_{}", std::process::id()));
    let b = dir.join(format!("stryke_vm_hl_b_{}", std::process::id()));
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    std::fs::write(&a, b"shared").expect("write");
    let sa = a.to_string_lossy();
    let sb = b.to_string_lossy();
    let src = format!(
        r#"link "{sa}", "{sb}";
        (slurp "{sb}") eq "shared" ? 1 : 0;"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "link() should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

#[cfg(unix)]
#[test]
fn try_vm_execute_lstat_symlink_size_differs_from_stat() {
    use std::os::unix::fs::symlink;
    let base = std::env::temp_dir().join(format!("stryke_vm_lstat_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("mkdir");
    let target = base.join("longtargetfilename");
    std::fs::write(&target, b"z").expect("write target");
    let link = base.join("L");
    symlink("longtargetfilename", &link).expect("symlink");
    let sl = link.to_string_lossy();
    let src = format!(
        r#"my @st = stat("{sl}");
        my @l = lstat("{sl}");
        $st[7] * 100 + $l[7];"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "stat/lstat should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 118);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn try_vm_execute_unlink_removes_file() {
    let dir = std::env::temp_dir();
    let path = dir.join("stryke_vm_unlink_test");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, b"x").expect("write temp");
    let ps = path.to_string_lossy();
    let src = format!(r#"unlink "{ps}";"#);
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "unlink should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

#[test]
fn try_vm_execute_wantarray_scalar_vs_list_in_sub() {
    let p = parse(
        r#"sub wa { wantarray ? 5 : 9 }
        my $s = wa()
        my @L = wa()
        $s * 100 + $L[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "wantarray in sub should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 905);
}

#[test]
fn try_vm_execute_rename_file() {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let p1 = dir.join(format!("stryke_vm_rename_from_{pid}"));
    let p2 = dir.join(format!("stryke_vm_rename_to_{pid}"));
    let _ = std::fs::remove_file(&p1);
    let _ = std::fs::remove_file(&p2);
    std::fs::write(&p1, b"mv").expect("write temp");
    let s1 = p1.to_string_lossy();
    let s2 = p2.to_string_lossy();
    let src = format!(
        r#"rename "{s1}", "{s2}";
        length(slurp "{s2}");"#,
    );
    let p = parse(&src).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "rename should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
    let _ = std::fs::remove_file(&p2);
}

#[test]
fn try_vm_execute_srand_makes_rand_repeatable() {
    let p = parse(
        r#"srand(4242)
        my $a = int(rand(100_000))
        srand(4242)
        my $b = int(rand(100_000))
        ($a == $b) ? 1 : 0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "srand and rand should compile on VM (CallBuiltin)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

#[test]
fn try_vm_execute_lc_uc_concat() {
    let p = parse(r#"lc("Ab") . uc("cD")"#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "lc and uc should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "abCD");
}

#[test]
fn try_vm_execute_scalar_reverse_string() {
    let p = parse(r#"scalar reverse "Perl""#).expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar reverse on string should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "lreP");
}

#[test]
fn try_vm_execute_chop_shortens_string() {
    let p = parse(
        r#"my $g = "xy"
        chop $g
        $g"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "chop should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "x");
}

/// `use overload` on `+` / `""` stays on the bytecode VM (no tree fallback).
#[test]
fn try_vm_execute_use_overload_add_and_qq_stringify() {
    let p = parse(
        r#"
        package O
        use overload '+' => 'add', '""' => 'str'
        sub add { my ($a, $b) = @_; $a->{n} + $b }
        sub str { "" . $_0->{n} }
        package main
        my $x = O->new(n => 3)
        "$x" . ":" . ($x + 1)
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "overload + and qq stringify should compile on VM path"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "3:4");
}

/// Unary `-` with `use overload 'neg'` stays on the VM (same path as `Op::Negate`).
#[test]
fn try_vm_execute_use_overload_unary_neg() {
    let p = parse(
        r#"
        package O
        use overload 'neg' => 'negate'
        sub negate { 77 }
        package main
        my $o = bless {}, "O"
        -$o
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "overload neg should compile on VM path");
    assert_eq!(out.unwrap().expect("vm").to_int(), 77);
}

/// Overloaded `.` when the plain string is on the left (Perl reverses args for the method).
#[test]
fn try_vm_execute_use_overload_concat_string_on_lhs() {
    let p = parse(
        r#"
        package O
        use overload '.' => 'odot'
        sub odot { my ($a, $b) = @_; "[" . $a->{n} . "+" . $b . "]" }
        package main
        my $a = O->new(n => "x")
        "z" . $a
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "overload concat (string . object) should compile on VM path"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "[x+z]");
}

/// `sprintf "%s"` uses overload stringify on the VM (matches tree `stringify_value`).
#[test]
fn try_vm_execute_sprintf_percent_s_overload_stringify() {
    let p = parse(
        r#"
        package O
        use overload '""' => 'as_string'
        sub as_string { "QQ" }
        package main
        my $o = bless {}, "O"
        sprintf "%s:%s", $o, "ok"
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "sprintf %s with overloaded object should stay on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "QQ:ok");
}

/// `join` uses overload stringify on the VM (`BuiltinId::Join` → `stringify_value`).
#[test]
fn try_vm_execute_join_overload_stringify() {
    let p = parse(
        r#"
        package O
        use overload '""' => 'as_str'
        sub as_str { "[" . $_0->{k} . "]" }
        package main
        my $o = bless { k => 9 }, "O"
        join "-", $o, "z"
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "join with overloaded elt should stay on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "[9]-z");
}

/// `!$obj` with `use overload 'bool'` — exercises `Op::LogNot` overload dispatch.
#[test]
fn try_vm_execute_use_overload_bool_unary_not() {
    let p = parse(
        r#"
        package O
        use overload 'bool' => 'as_bool'
        sub as_bool { $_0->{f} }
        package main
        my $o = bless { f => 0 }, "O"
        !$o
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "overload bool + ! should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

#[test]
fn try_vm_execute_use_overload_not_keyword_with_bool() {
    let p = parse(
        r#"
        package O
        use overload 'bool' => 'as_bool'
        sub as_bool { $_0->{f} }
        package main
        my $o = bless { f => 1 }, "O"
        not $o
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "overload bool + not EXPR should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

/// `nomethod` fallback for a missing `+` overload (VM binop dispatch).
#[test]
fn try_vm_execute_use_overload_nomethod_binop() {
    let p = parse(
        r#"
        package O
        use overload nomethod => 'catch_all', fallback => 1
        sub catch_all { my ($a, $b, $op) = @_; $op eq "+" ? 88 : 0 }
        package main
        my $x = bless { n => 1 }, "O"
        my $y = bless { n => 2 }, "O"
        $x + $y
    "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "nomethod binop should stay on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 88);
}

/// qq with `$a[0]` — `StringPart::Expr` array-element lowering + stringify.
#[test]
fn try_vm_execute_qq_named_array_element() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (33, 44)
        "n$a[0]""#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "qq array element should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "n33");
}

/// Braced scalar plus trailing literal — extra `Op::Concat` after interpolation.
#[test]
fn try_vm_execute_qq_braced_scalar_trailing_literal() {
    let p = parse(
        r#"no strict 'vars'
        my $u = 8
        "k${u}zz""#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "qq braced scalar + literal should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "k8zz");
}

/// `@a[i,j] = (v1,v2)` — element-wise assign (`Op::SetNamedArraySlice`).
#[test]
fn try_vm_execute_named_array_slice_list_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 2)
        @a[0, 1] = (30, 40)
        $a[0] + $a[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named array multi-assign list should compile (SetNamedArraySlice)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 70);
}

#[test]
fn try_vm_execute_qq_braced_scalar_leading_and_trailing_literals() {
    let p = parse(
        r#"no strict 'vars'
        my $u = 4
        "M${u}N""#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "qq literal + braced scalar + literal should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "M4N");
}

#[test]
fn try_vm_execute_qq_mixed_braced_and_plain_scalar() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 3
        my $y = 4
        "p${x}q$y""#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "qq mixing braced and plain scalar parts should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "p3q4");
}

/// Single index still uses [`Op::SetNamedArraySlice`] with `n == 1`.
#[test]
fn try_vm_execute_named_array_slice_single_index_list_rhs() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 2)
        @a[1] = (99)
        $a[0] + $a[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@a[i] = (v) with one index should compile (SetNamedArraySlice)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 100);
}

#[test]
fn try_vm_execute_named_array_slice_three_indices_list_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 1, 1)
        @a[0, 1, 2] = (2, 3, 4)
        $a[0] * 100 + $a[1] * 10 + $a[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named slice with three indices should compile (SetNamedArraySlice)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 234);
}

/// qq with a leading literal and two scalars (`"p$x$y"`) — exercises multi-`Concat` lowering.
#[test]
fn try_vm_execute_qq_literal_then_two_scalars() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 2
        my $y = 3
        "p$x$y""#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "literal-first qq with two scalars should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "p23");
}

#[test]
fn try_vm_execute_scalar_defined_or_assign() {
    let p = parse(
        r#"my $x
        $x //= 99
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$x //= should compile (GetScalar + JumpIfDefinedKeep + SetScalar*Keep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 99);
}

#[test]
fn try_vm_execute_scalar_defined_or_assign_short_circuit() {
    let p = parse(
        r#"my $x = 0
        my $runs = 0
        $x //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "$x //= should skip RHS when LHS is defined");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_scalar_log_or_assign() {
    let p = parse(
        r#"my $x = 0
        $x ||= 8
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$x ||= should compile (GetScalar + JumpIfTrueKeep + SetScalar*Keep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 8);
}

#[test]
fn try_vm_execute_scalar_log_or_assign_short_circuit() {
    let p = parse(
        r#"my $x = 5
        my $runs = 0
        $x ||= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "$x ||= should skip RHS when LHS is true");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_scalar_log_and_assign() {
    let p = parse(
        r#"my $x = 2
        $x &&= 7
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar &&= should compile (JumpIfFalseKeep + SetScalar*Keep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 7);
}

#[test]
fn try_vm_execute_scalar_log_and_assign_short_circuit() {
    let p = parse(
        r#"my $x = 0
        my $runs = 0
        $x &&= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar &&= should skip RHS when LHS is false"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_array_elem_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a
        $a[0] //= 11
        $a[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "array element //= should compile (SetArrayElemKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 11);
}

#[test]
fn try_vm_execute_array_elem_defined_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (9)
        my $runs = 0
        $a[0] //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "array element //= should skip RHS when LHS is defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_array_elem_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (0)
        $a[0] ||= 6
        $a[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "array element ||= should compile (SetArrayElemKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_execute_hash_elem_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my %h
        $h{"x"} //= 33
        $h{"x"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "hash element //= should compile (SetHashElemKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 33);
}

#[test]
fn try_vm_execute_hash_elem_defined_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("x" => 1)
        my $runs = 0
        $h{"x"} //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "hash element //= should skip RHS when LHS is defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_hash_elem_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("x" => 0)
        $h{"x"} ||= 4
        $h{"x"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "hash element ||= should compile (SetHashElemKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 4);
}

#[test]
fn try_vm_execute_array_elem_log_and_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1)
        $a[0] &&= 8
        $a[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "array element &&= should compile (JumpIfFalseKeep + SetArrayElemKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 8);
}

#[test]
fn try_vm_execute_hash_elem_log_and_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("x" => 0)
        my $runs = 0
        $h{"x"} &&= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "hash element &&= should skip RHS when LHS is false"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_log_and_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1 }
        $h->{"a"} &&= 9
        $h->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash &&= should compile (JumpIfFalseKeep + SetArrowHashKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 9);
}

#[test]
fn try_vm_execute_indirect_coderef_call() {
    let p = parse("my $inc = fn { $_[0] + 1 }; $inc(41)").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "IndirectCall should compile (Op::IndirectCall), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_sort_with_coderef_comparator() {
    let p = parse(
        r#"no strict 'vars'
        my $cmp = fn { $a <=> $b }
        join(",", sort $cmp (3, 1, 2))"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "sort $coderef LIST should compile (Op::SortWithCodeComparator)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,2,3");
}

#[test]
fn try_vm_execute_symbolic_scalar_deref() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 42
        my $r = \$x
        $$r"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "symbolic scalar deref should compile (Op::SymbolicDeref), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 0
        my $r = \$x
        $$r = 7
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$$r = should compile (Op::SetSymbolicScalarRefKeep), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 7);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_compound_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 10
        my $r = \$x
        $$r += 2
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$$r += should compile (SymbolicDeref + SetSymbolicScalarRef)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 12);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $x
        my $r = \$x
        $$r //= 99
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$$r //= should compile (JumpIfDefinedKeep + SetSymbolicScalarRefKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 99);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_defined_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 0
        my $r = \$x
        my $runs = 0
        $$r //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "$$r //= should skip RHS when LHS is defined");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 0
        my $r = \$x
        $$r ||= 8
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$$r ||= should compile (JumpIfTrueKeep + SetSymbolicScalarRefKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 8);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_log_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 5
        my $r = \$x
        my $runs = 0
        $$r ||= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "$$r ||= should skip RHS when LHS is true");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_compound_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 10 }
        $h->{"a"} += 2
        $h->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash compound assign should compile (Dup2 + ArrowHash + SetArrowHash)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 12);
}

#[test]
fn try_vm_execute_arrow_hash_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => undef }
        $h->{"a"} //= 42
        $h->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash //= should compile (JumpIfDefinedKeep + SetArrowHashKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_arrow_hash_defined_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1 }
        my $runs = 0
        $h->{"a"} //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash //= should skip RHS when LHS is defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_scalar_deref_hash_defined_or_assign() {
    let p = parse(
        r#"my %h = ()
        my $r = \%h
        $$r{x} //= 42
        $h{x}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "hash-element defined-or assign on VM (\\%h via \\$r)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_scalar_deref_hash_defined_or_assign_short_circuit() {
    let p = parse(
        r#"my %h = (a => 1)
        my $r = \%h
        my $runs = 0
        $$r{a} //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "hash-element defined-or should skip RHS when defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 0 }
        $h->{"a"} ||= 9
        $h->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash ||= should compile (JumpIfTrueKeep + SetArrowHashKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 9);
}

#[test]
fn try_vm_execute_arrow_hash_log_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 2 }
        my $runs = 0
        $h->{"a"} ||= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash ||= should skip RHS when LHS is true"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1 }
        $h->{"b"} = 2
        $h->{"a"} + $h->{"b"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash assign should compile (Op::SetArrowHashKeep), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_arrow_hash_assign_returns_rhs() {
    let p = parse(
        r#"no strict 'vars'
        my $h = {}
        my $x = ($h->{"k"} = 11)
        $x + $h->{"k"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash assignment expression should yield RHS"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 22);
}

#[test]
fn try_vm_execute_arrow_array_assign_returns_rhs() {
    let p = parse(
        r#"no strict 'vars'
        my $a = []
        my $x = ($a->[0] = 4)
        $x + $a->[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array assignment expression should yield RHS"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 8);
}

#[test]
fn try_vm_execute_arrow_array_compound_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20]
        $a->[0] += 2
        $a->[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array compound assign should compile (Dup2 + ArrowArray + SetArrowArray)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 12);
}

#[test]
fn try_vm_execute_arrow_array_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [undef]
        $a->[0] //= 7
        $a->[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array //= should compile (JumpIfDefinedKeep + SetArrowArrayKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 7);
}

#[test]
fn try_vm_execute_arrow_array_defined_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [99]
        my $runs = 0
        $a->[0] //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array //= should skip RHS when LHS is defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_array_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [0]
        $a->[0] ||= 5
        $a->[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array ||= should compile (JumpIfTrueKeep + SetArrowArrayKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 5);
}

#[test]
fn try_vm_execute_arrow_array_log_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [3]
        my $runs = 0
        $a->[0] ||= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array ||= should skip RHS when LHS is true"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

/// `$r->[ .. ] //=` with a range / list subscript — uses [`Op::ArrowArraySlicePeekLast`](1) like multi-index slices.
#[test]
fn try_vm_execute_empty_named_array_slice_assign_vm_runtime_error() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 2)
        @a[] = (3, 4)
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for @a[] =, got {:?}", out);
    };
    assert!(
        e.message.contains("assign to empty array slice"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_execute_empty_named_hash_slice_assign_vm_runtime_error() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("a", 1)
        @h{} = (2)
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for @h{{}} =, got {:?}", out);
    };
    assert!(
        e.message.contains("assign to empty hash slice"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_execute_empty_arrow_array_slice_assign_vm_runtime_error() {
    // `1..0` yields no indices (same as an empty slice through the ref).
    let p = parse(
        r#"no strict 'vars'
        my $r = [1, 2]
        @$r[1..0] = (3, 4)
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for @$r[] =, got {:?}", out);
    };
    assert!(
        e.message.contains("assign to empty array slice"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_execute_empty_hash_slice_deref_assign_vm_runtime_error() {
    // Empty braces: zero keys in the slice (not `@$r{()}` — `()` is one scalar key, `undef`).
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1 }
        my $r = $h
        @$r{} = (2)
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for @$r{{}} =, got {:?}", out);
    };
    assert!(
        e.message.contains("assign to empty hash slice"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_execute_empty_hash_slice_deref_plus_eq_vm_runtime_error() {
    let p = parse(
        r#"no strict 'vars'
        my $h = {}
        my $r = $h
        @$r{} += 1
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for @$r{{}} +=, got {:?}", out);
    };
    assert!(
        e.message.contains("assign to empty hash slice"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_compile_exists_delete_href_emits_arrow_hash_ops() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $h = {}
                my $r = $h
                exists $r->{k}
                delete $r->{k}"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::ExistsArrowHashElem)),
        "expected ExistsArrowHashElem in {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::DeleteArrowHashElem)),
        "expected DeleteArrowHashElem in {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_exists_delete_href() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { u => 1 }
        my $r = $h
        my $e = exists $r->{u}
        my $d = delete $r->{u}
        my $e2 = exists $r->{u}
        $e . "," . $d . "," . $e2"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "exists/delete on hashref element should compile (ExistsArrowHashElem / DeleteArrowHashElem)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,1,0");
}

#[test]
fn try_vm_compile_exists_delete_named_array_emits_array_elem_ops() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"my @a = (1, 2, 3)
                exists $a[1]
                delete $a[1]"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::ExistsArrayElem(_))),
        "expected ExistsArrayElem in {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::DeleteArrayElem(_))),
        "expected DeleteArrayElem in {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::ExistsExpr(_))),
        r"exists $a[i] should not use ExistsExpr pool"
    );
}

#[test]
fn try_vm_execute_exists_delete_named_array() {
    let p = parse(
        r#"my @a = (10, 20, 30)
        my $e0 = exists $a[99]
        my $e1 = exists $a[1]
        my $d = delete $a[1]
        my $e2 = exists $a[1]
        $e0 . "," . $e1 . "," . $d . "," . $e2"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "exists/delete on named array element should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "0,1,20,1");
}

#[test]
fn try_vm_compile_exists_delete_aref_emits_arrow_array_ops() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1, 2, 3]
                exists $a->[1]
                delete $a->[1]"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::ExistsArrowArrayElem)),
        "expected ExistsArrowArrayElem in {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::DeleteArrowArrayElem)),
        "expected DeleteArrowArrayElem in {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::ExistsExpr(_))),
        "exists on aref->[i] should not use ExistsExpr pool"
    );
}

#[test]
fn try_vm_execute_exists_delete_aref() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20, 30]
        my $e0 = exists $a->[99]
        my $e1 = exists $a->[1]
        my $d = delete $a->[1]
        my $e2 = exists $a->[1]
        $e0 . "," . $e1 . "," . $d . "," . $e2"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "exists/delete on array ref element should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "0,1,20,1");
}

#[test]
fn try_vm_compile_keys_values_href_use_from_value_ops() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $h = { a => 1, b => 2 }
                my $r = $h
                scalar keys $r
                scalar values $r"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::KeysFromValueScalar)),
        "expected KeysFromValueScalar, got {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::ValuesFromValueScalar)),
        "expected ValuesFromValueScalar, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::KeysExpr(_))),
        "should not use KeysExpr pool for keys on hashref, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_keys_values_hashref_scalar_context() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { x => 1, y => 2, z => 3 }
        my $r = $h
        (scalar keys $r) + (scalar values $r)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "keys/values on hashref should compile (KeysFromValueScalar / ValuesFromValueScalar)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_compile_push_aref_uses_push_array_deref() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1]
                push @$a, 2, 3"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk.ops.iter().any(|o| matches!(o, Op::PushArrayDeref)),
        "expected PushArrayDeref, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::PushExpr(_))),
        "push @$a should not use PushExpr pool"
    );
}

#[test]
fn try_vm_compile_scalar_at_aref_uses_array_deref_len() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1, 2, 3]
                scalar @$a"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk.ops.iter().any(|o| matches!(o, Op::ArrayDerefLen)),
        "expected ArrayDerefLen for scalar @$a, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SymbolicDeref(b) if *b == 1)),
        "scalar @$a should not use SymbolicDeref(Array), got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_scalar_at_aref_is_length() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20, 30]
        scalar @$a"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar @$a should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_scalar_at_aref_empty_is_zero() {
    let p = parse(
        r#"no strict 'vars'
        my $a = []
        scalar @$a"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar @$a on empty array ref should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_compile_scalar_braced_aref_uses_array_deref_len() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [9, 8]
                scalar @{$a}"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk.ops.iter().any(|o| matches!(o, Op::ArrayDerefLen)),
        "expected ArrayDerefLen for scalar @{{$a}}, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_scalar_braced_aref_is_length() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [1, 2, 3, 4, 5]
        scalar @{$a}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar @{{$a}} should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 5);
}

#[test]
fn try_vm_execute_scalar_braced_sub_returning_aref_is_length() {
    let p = parse(
        r#"no strict 'vars'
        sub mk { [1, 2, 3, 4] }
        scalar @{mk()}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar @{{sub()}} array deref should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 4);
}

#[test]
fn try_vm_execute_assignment_rhs_at_aref_yields_length() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [9, 8, 7]
        my $n = @$a
        $n"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "my $n = @$a should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_assign_named_array_to_scalar_is_length() {
    let p = parse(
        r#"my @y = (10, 20, 30)
        my $x = @y
        $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "my $x = @y should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_compile_scalar_percent_hash_uses_value_scalar_context() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"my %h = (a => 1, b => 2)
                scalar %h"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::ValueScalarContext)),
        "expected ValueScalarContext after GetHash for scalar %h, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_scalar_percent_empty_hash_is_zero() {
    let p = parse(
        r#"my %h = ()
        scalar %h"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some());
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_compile_scalar_percent_href_emits_hash_deref_and_value_scalar_context() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $r = { a => 1, b => 2 }
                scalar %$r"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk.ops.iter().any(|o| matches!(o, Op::SymbolicDeref(2))),
        "expected SymbolicDeref(Hash) for %$r, got {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::ValueScalarContext)),
        "expected ValueScalarContext after hash deref for scalar %$r, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_scalar_percent_href_nonempty_fill_string() {
    let p = parse(
        r#"no strict 'vars'
        my $r = { a => 1, b => 2 }
        scalar %$r"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar %$r should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "2/3");
}

#[test]
fn try_vm_execute_scalar_percent_href_empty_is_zero() {
    let p = parse(
        r#"no strict 'vars'
        my $r = {}
        scalar %$r"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some());
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_compile_scalar_named_array_emits_array_len_not_get_array() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"my @t = (9, 8)
                scalar @t"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk.ops.iter().any(|o| matches!(o, Op::ArrayLen(_))),
        "expected ArrayLen for scalar @t, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::GetArray(_))),
        "scalar @t should not load full array via GetArray, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_scalar_named_array_length() {
    let p = parse(
        r#"my @u = (1, 2, 3, 4)
        scalar @u"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar @u should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 4);
}

#[test]
fn try_vm_execute_join_scalar_at_aref_single_argument() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20, 30, 40, 50]
        join "-", scalar @$a"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "join with scalar @$a should compile on VM (one list element)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "5");
}

#[test]
fn try_vm_compile_splice_aref_uses_splice_array_deref() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [10, 20, 30, 40]
                join("-", splice @$a, 1, 2)"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SpliceArrayDeref(n) if *n == 0)),
        "expected SpliceArrayDeref(0) for splice with no replacement list, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::SpliceExpr(_))),
        "splice @$a with small replacement count should not use SpliceExpr pool"
    );
}

#[test]
fn try_vm_compile_splice_aref_with_replacements_emits_splice_array_deref_count() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1, 2, 3, 4]
                join("-", splice @$a, 1, 2, 9, 8)"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SpliceArrayDeref(n) if *n == 2)),
        "expected SpliceArrayDeref(2) for two replacement elems, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_compile_splice_aref_negative_offset_emits_splice_array_deref() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1, 2, 3, 4, 5]
                join("-", splice @$a, -2)"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SpliceArrayDeref(n) if *n == 0)),
        "expected SpliceArrayDeref(0) for splice @$a with negative OFFSET, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::SpliceExpr(_))),
        "small splice @$a should not fall back to SpliceExpr pool, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_compile_splice_aref_negative_length_emits_splice_array_deref() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1, 2, 3, 4, 5]
                join("-", splice @$a, 0, -2)"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SpliceArrayDeref(n) if *n == 0)),
        "expected SpliceArrayDeref(0) for splice @$a with negative LENGTH, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::SpliceExpr(_))),
        "splice @$a, 0, -2 should not use SpliceExpr pool, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_compile_splice_aref_negative_offset_with_replacement_emits_splice_array_deref_count() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [1, 2, 3, 4, 5]
                join("-", splice @$a, -2, 1, 99)"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SpliceArrayDeref(n) if *n == 1)),
        "expected SpliceArrayDeref(1) for one replacement with negative OFFSET, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::SpliceExpr(_))),
        "splice @$a, -2, 1, 99 should not use SpliceExpr pool, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_compile_splice_aref_three_replacements_emits_splice_array_deref_count() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(
            &parse(
                r#"no strict 'vars'
                my $a = [10, 20, 30, 40]
                join("-", splice @$a, 1, 2, 1, 2, 3)"#,
            )
            .expect("parse"),
        )
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::SpliceArrayDeref(n) if *n == 3)),
        "expected SpliceArrayDeref(3) for three replacement elems, got {:?}",
        chunk.ops
    );
    assert!(
        !chunk.ops.iter().any(|o| matches!(o, Op::SpliceExpr(_))),
        "three-elem replacement splice @$a should not use SpliceExpr pool, got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_splice_aref_returns_removed_slice() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [10, 20, 30, 40]
        join("-", splice @$v, 1, 2)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "splice on array ref should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "20-30");
}

#[test]
fn try_vm_execute_splice_aref_negative_offset_removed_tail() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4, 5]
        my $r = join "-", splice @$v, -2
        $r . "|" . join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "splice @$v with negative OFFSET should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "4-5|1-2-3");
}

#[test]
fn try_vm_execute_splice_aref_negative_length_preserves_tail() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4, 5]
        my $r = join "-", splice @$v, 0, -2
        $r . "|" . join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "splice @$v with negative LENGTH should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-2-3|4-5");
}

#[test]
fn try_vm_execute_splice_aref_negative_offset_with_replacement() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4, 5]
        splice @$v, -2, 1, 99
        join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "splice @$v with negative OFFSET and replacement should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-2-3-99-5");
}

#[test]
fn try_vm_execute_splice_aref_three_replacements_mutates_target() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [10, 20, 30, 40]
        splice @$v, 1, 2, 1, 2, 3
        join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "splice @$v with three replacements should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "10-1-2-3-40");
}

#[test]
fn try_vm_execute_splice_aref_zero_length_insert() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3]
        splice @$v, 1, 0, 9
        join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "splice LENGTH0 insert should compile on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-9-2-3");
}

#[test]
fn try_vm_execute_push_aref_splice_list_chain() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3]
        push @$v, splice @$v, 0, 1
        join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "push + splice on same aref should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "2-3-1");
}

#[test]
fn try_vm_execute_splice_aref_with_replacements_mutates_target() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4]
        my $removed = join "-", splice @$v, 1, 2, 9, 8
        ($removed eq "2-3" && $v->[1] == 9 && $v->[2] == 8 && $v->[3] == 4) ? 1 : 0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "splice @$v with replacement list should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

#[test]
fn try_vm_execute_scalar_splice_aref_returns_last_removed() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [5, 6, 7]
        scalar splice @$a, 0, 2"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "scalar splice on array ref should compile on VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_execute_push_pop_shift_unshift_aref() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20]
        my $n = push @$a, 30
        my $p = pop @$a
        my $s = shift @$a
        my $u = unshift @$a, 5
        $n + $p + $s + $u + $a->[0] + $a->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "push/pop/shift/unshift on array ref should compile to deref ops"
    );
    // n=3 p=30 s=10 u=2 (unshift returns new length) a[0]=5 a[1]=20 => 70
    assert_eq!(out.unwrap().expect("vm").to_int(), 70);
}

#[test]
fn try_vm_execute_named_array_splice_negative_offset_replacement() {
    let p = parse(
        r#"my @a = (1, 2, 3, 4, 5)
        splice @a, -2, 1, 88
        join "-", @a"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "named splice with negative OFFSET on VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-2-3-88-5");
}

#[test]
fn try_vm_execute_unshift_splice_aref_chain() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3]
        unshift @$v, splice(@$v, 0, 1)
        join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "unshift with splice on same aref");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-2-3");
}

#[test]
fn try_vm_execute_grep_block_aref() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4]
        scalar grep { $_ > 1 } @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "grep block over array ref");
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_map_block_aref() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3]
        my @m = map { $_ * 2 } @$v
        $m[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "map block over array ref");
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_execute_scalar_keys_hashref() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { u => 1, v => 2, w => 3 }
        scalar keys %$h"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar keys on hashref");
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_for_loop_over_aref() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3]
        my $s = 0
        for my $x (@$v) { $s = $s + $x; }
        $s"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "for-loop over array ref");
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_execute_splice_aref_variable_offset() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4]
        my $o = 1
        splice @$v, $o, 2, 9
        join "-", @$v"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "splice on aref with scalar OFFSET variable");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-9-4");
}

#[test]
fn try_vm_execute_concat_two_arefs() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [1, 2]
        my $b = [3, 4]
        my @x = (@$a, @$b)
        join "-", @x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "list concat of two arefs");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-2-3-4");
}

#[test]
fn try_vm_execute_scalar_splice_named_negative_offset() {
    let p = parse(
        r#"my @a = (1, 2, 3, 4, 5)
        my $n = scalar splice @a, -3, 2
        $n . "|" . join("-", @a)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar splice named array negative OFFSET");
    assert_eq!(out.unwrap().expect("vm").to_string(), "4|1-2-5");
}

#[test]
fn try_vm_execute_splice_named_three_replacements_return_removed() {
    let p = parse(
        r#"my @a = (1, 2, 3, 4)
        my @b = splice @a, 1, 2, 9, 8, 7
        join("-", @a) . "|" . join("-", @b)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named splice with three replacements, list return"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-9-8-7-4|2-3");
}

#[test]
fn try_vm_execute_splice_aref_three_replacements_return_removed() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3, 4]
        my @b = splice @$v, 1, 2, 9, 8, 7
        join("-", @$v) . "|" . join("-", @b)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "aref splice three replacements, list return");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1-9-8-7-4|2-3");
}

#[test]
fn try_vm_execute_shift_then_pop_aref() {
    let p = parse(
        r#"no strict 'vars'
        my $v = [1, 2, 3]
        shift @$v
        my $x = pop @$v
        $x . "|" . join("-", @$v)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "shift then pop on same aref");
    assert_eq!(out.unwrap().expect("vm").to_string(), "3|2");
}

#[test]
fn try_vm_execute_empty_arrow_array_slice_preinc_vm_runtime_error() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [1]
        ++@$r[1..0]
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for ++@$r[1..0], got {:?}", out);
    };
    assert!(
        e.message
            .contains("array slice increment needs at least one index"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_execute_empty_array_slice_preinc_vm_runtime_error() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1)
        ++@a[]
        0"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    let Some(Err(e)) = out else {
        panic!("expected VM runtime error for ++@a[], got {:?}", out);
    };
    assert!(
        e.message
            .contains("array slice increment needs at least one index"),
        "msg={:?}",
        e.message
    );
}

#[test]
fn try_vm_compile_empty_array_slice_plus_eq_no_tree_fallback() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(&parse("no strict 'vars'; my @a; @a[] += 1").expect("parse"))
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::NamedArraySliceCompound(_, _, n) if *n == 0)),
        "expected NamedArraySliceCompound(_, _, 0), got {:?}",
        chunk.ops
    );
}

#[test]
fn try_vm_execute_arrow_array_defined_or_range_subscript() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [1, undef]
        $a->[0..1] //= (10, 20)
        $a->[0] . "," . $a->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$r->[RANGE] //= should compile (ArrowArraySlicePeekLast + SetArrowArraySliceLastKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,20");
}

#[test]
fn try_vm_execute_arrow_array_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [1, 2]
        $a->[2] = 3
        $a->[0] + $a->[1] + $a->[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array assign should compile (Op::SetArrowArrayKeep), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_execute_arrow_array_slice_through_atref_read() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20, 30]
        my $r = $a
        join(",", @$r[1,2])"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@$aref[i,j] read should compile (Op::ArrowArraySlice + array ref base)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "20,30");
}

#[test]
fn try_vm_execute_arrow_array_slice_through_atref_read_single() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [10, 20]
        my $r = $a
        @$r[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@$aref[i] read should use array ref base (ArrowArray), not symbolic @ expansion"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 20);
}

#[test]
fn try_vm_execute_arrow_array_slice_through_atref_assign_list() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [0, 0, 0, 0]
        my $r = $a
        @$r[1,2] = (7, 8)
        $r->[1] . "," . $r->[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@$aref[i,j] = (v1,v2) should compile (SetArrowArray per element)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "7,8");
}

#[test]
fn try_vm_execute_arrow_array_pre_inc_only() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [9]
        ++$a->[0]
        $a->[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 10);
}

#[test]
fn try_vm_execute_arrow_hash_pre_inc_only() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "x" => 9 }
        ++$h->{"x"}
        $h->{"x"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 10);
}

#[test]
fn try_vm_execute_arrow_array_hash_pre_post_inc() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [9]
        my $h = { "x" => 9 }
        my $pre_a = ++$a->[0]
        my $post_a = $a->[0]++
        my $pre_h = ++$h->{"x"}
        my $post_h = $h->{"x"}++
        $pre_a + $post_a + $a->[0] + $pre_h + $post_h + $h->{"x"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "pre/post ++ on arrow array+hash should compile (SetArrow* + Arrow*Postfix)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 62);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_pre_post_inc() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 9
        my $r = \$x
        my $pre = ++$$r
        my $post = $$r++
        $pre + $post + $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "++/-- on $$r should compile (SymbolicDeref + SetSymbolicScalarRefKeep / SymbolicScalarRefPostfix)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 31);
}

#[test]
fn try_vm_execute_symbolic_array_hash_ref_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $a = [1, 2]
        my $r = $a
        @{ $r } = (3, 4, 5)
        my $h = { "a" => 1 }
        my $hr = $h
        %{ $hr } = ("b", 2, "c", 3)
        join(",", @$r) . ";" . join(",", sort keys %$hr)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "symbolic array/hash deref assign should compile (SetSymbolicArrayRef / SetSymbolicHashRef)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "3,4,5;b,c");
}

#[test]
fn try_vm_execute_hash_slice_deref() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { a => 10, b => 20 }
        my $r = $h
        join(",", @$r{"a", "b"})"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@$href{{keys}} should compile (Op::HashSliceDeref), not tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,20");
}

#[test]
fn try_vm_execute_hash_slice_deref_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1, "b" => 2 }
        my $r = $h
        @$r{"a", "b"} = (10, 20)
        $r->{"a"} . "," . $r->{"b"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@$href{{keys}} = should compile (Op::SetHashSliceDeref)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,20");
}

/// `%name{k1,k2} = LIST` — [`Op::SetHashSlice`] (stash hash, element-wise like `@$href{...}`).
#[test]
fn try_vm_execute_named_hash_slice_assign() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("a", 1, "b", 2, "c", 3)
        @h{qw(a c)} = (100, 300)
        $h{"a"} . "," . $h{"b"} . "," . $h{"c"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "%h{{k1,k2}} = should compile (SetHashSlice)");
    assert_eq!(out.unwrap().expect("vm").to_string(), "100,2,300");
}

/// Multi-key `@h{k1,k2} += EXPR` — [`Op::NamedHashSliceCompound`]; only the last key is updated.
#[test]
fn try_vm_execute_named_hash_slice_compound_assign_multi_key() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("a", 10, "b", 20)
        @h{"a","b"} += 5
        my $first = $h{"a"}
        my $second = defined($h{"b"}) ? 1 : 0
        $first * 10 + $second"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-key @h{{k1,k2}} += should compile (NamedHashSliceCompound)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 101);
}

#[test]
fn compile_named_hash_slice_multi_key_compound_emits_dedicated_op() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(&parse("my %h = (); @h{\"a\",\"b\"} += 1").expect("parse"))
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::NamedHashSliceCompound(_, _, n) if *n == 2)),
        "expected NamedHashSliceCompound(_, _, 2), got {:?}",
        chunk.ops
    );
}

/// Multi-key `@h{…} //=` tests only the **last** flattened key; assigns that slot from the list’s last element.
#[test]
fn try_vm_execute_named_hash_slice_multi_key_defined_or() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("a", 1, "b", undef)
        @h{qw(a b)} //= (10, 20)
        $h{"a"} . "," . $h{"b"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-key @h //= should compile (NamedHashSlicePeekLast + SetNamedHashSliceLastKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,20");
}

#[test]
fn compile_named_hash_slice_multi_key_logical_compound_emits_peek_and_set_last() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(&parse("my %h = (); @h{\"a\",\"b\"} //= 1").expect("parse"))
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::NamedHashSlicePeekLast(_, n) if *n == 2)),
        "expected NamedHashSlicePeekLast(_, 2), got {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| { matches!(o, Op::SetNamedHashSliceLastKeep(_, n) if *n == 2) }),
        "expected SetNamedHashSliceLastKeep(_, 2), got {:?}",
        chunk.ops
    );
}

/// Multi-key `@$href{…} //=` — [`Op::HashSliceDerefPeekLast`] + [`Op::HashSliceDerefSetLastKeep`].
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_defined_or() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1, "b" => undef }
        my $r = $h
        @$r{qw(a b)} //= (10, 20)
        $r->{"a"} . "," . $r->{"b"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-key @$href //= should compile (HashSliceDerefPeekLast)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,20");
}

#[test]
fn compile_hash_slice_deref_multi_key_logical_compound_emits_peek_and_set_last() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(&parse("my %h = (); my $r = \\%h; @$r{\"a\",\"b\"} //= 1").expect("parse"))
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::HashSliceDerefPeekLast(n) if *n == 2)),
        "expected HashSliceDerefPeekLast(2), got {:?}",
        chunk.ops
    );
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| { matches!(o, Op::HashSliceDerefSetLastKeep(n) if *n == 2) }),
        "expected HashSliceDerefSetLastKeep(2), got {:?}",
        chunk.ops
    );
}

/// `++@h{k1,k2,k3}` on a stash hash — [`Op::NamedHashSliceIncDec`].
#[test]
fn try_vm_execute_named_hash_slice_multi_key_pre_inc() {
    let p = parse(
        r#"no strict 'vars'
        my %h = ("a", 10, "b", 20, "c", 30)
        my $pre = ++@h{"a","b","c"}
        my $first = $h{"a"}
        my $second_def = defined($h{"b"}) ? 1 : 0
        $pre * 100 + $first * 10 + $second_def"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "++ on multi-key @h{{k1,k2,k3}} should compile (NamedHashSliceIncDec)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3201);
}

#[test]
fn compile_named_hash_slice_multi_key_inc_dec_emits_dedicated_op() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let cases = [
        ("++@h{\"a\",\"b\"};", 0u8),
        ("--@h{\"a\",\"b\"};", 1u8),
        ("@h{\"a\",\"b\"}++;", 2u8),
        ("@h{\"a\",\"b\"}--;", 3u8),
    ];
    for (tail, expected_kind) in cases {
        let src = format!("my %h = (); {}", tail);
        let chunk = Compiler::new()
            .compile_program(&parse(&src).expect("parse"))
            .expect("compile");
        assert!(
            chunk.ops.iter().any(
                |o| matches!(o, Op::NamedHashSliceIncDec(k, _, n) if *k == expected_kind && *n == 2)
            ),
            "expected NamedHashSliceIncDec({}, _, 2) for {:?}, got {:?}",
            expected_kind,
            tail,
            chunk.ops
        );
    }
}

#[test]
fn try_vm_execute_hash_slice_deref_compound_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 10 }
        my $r = $h
        @$r{"a"} += 2
        $r->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "single-key @$href{{\"k\"}} += should compile (Dup2 + ArrowHash + SetArrowHash)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 12);
}

/// Multi-key `@$href{k1,k2} += EXPR` goes through [`Op::HashSliceDerefCompound`]; Perl 5 updates only
/// the **last** key (`$b` becomes 25, `$a` unchanged).
#[test]
fn try_vm_execute_hash_slice_deref_compound_assign_multi_key() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 10, "b" => 20 }
        my $r = $h
        @$r{"a","b"} += 5
        my $first = $r->{"a"}
        my $second = defined($r->{"b"}) ? 1 : 0
        $first * 10 + $second"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-key @$href{{k1,k2}} += should compile (HashSliceDerefCompound)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 101);
}

/// Ensure the multi-key compound assign emits [`Op::HashSliceDerefCompound`], not a tree fallback.
#[test]
fn compile_hash_slice_deref_multi_key_compound_emits_dedicated_op() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(&parse("my %h = (); my $r = \\%h; @$r{\"a\",\"b\"} += 1").expect("parse"))
        .expect("compile");
    assert!(
        chunk
            .ops
            .iter()
            .any(|o| matches!(o, Op::HashSliceDerefCompound(_, n) if *n == 2)),
        "expected HashSliceDerefCompound(_, 2), got {:?}",
        chunk.ops
    );
}

/// `++@$href{k1,k2}` on a multi-key slice: uses [`Op::HashSliceDerefIncDec`] (kind=0); only the last
/// key is incremented (Perl 5).
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_pre_inc() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 10, "b" => 20, "c" => 30 }
        my $r = $h
        my $pre = ++@$r{"a","b","c"}
        my $first = $r->{"a"}
        my $second_def = defined($r->{"b"}) ? 1 : 0
        $pre * 100 + $first * 10 + $second_def"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "++ on multi-key @$href{{k1,k2,k3}} should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3201);
}

/// `@$href{k1,k2}++` (postfix): returns the old **last** element; only that key is incremented.
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_post_inc() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 10, "b" => 20, "c" => 30 }
        my $r = $h
        my $post = @$r{"a","b","c"}++
        my $first = $r->{"a"}
        my $second_def = defined($r->{"b"}) ? 1 : 0
        $post . ":" . $first . ":" . $second_def"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "postfix ++ on multi-key @$href{{k1,k2,k3}} should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "30:10:1");
}

/// Multi-key `--` (postfix): only the last key is decremented.
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_post_dec() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 100, "b" => 200 }
        my $r = $h
        @$r{"a","b"}--
        $r->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "postfix -- on multi-key slice should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 100);
}

/// Ensure all four multi-key ++/-- forms emit [`Op::HashSliceDerefIncDec`] with the right kind byte.
#[test]
fn compile_hash_slice_deref_multi_key_inc_dec_emits_dedicated_op() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let cases = [
        ("++@$r{\"a\",\"b\"};", 0u8),
        ("--@$r{\"a\",\"b\"};", 1u8),
        ("@$r{\"a\",\"b\"}++;", 2u8),
        ("@$r{\"a\",\"b\"}--;", 3u8),
    ];
    for (tail, expected_kind) in cases {
        let src = format!("my %h = (); my $r = \\%h; {}", tail);
        let chunk = Compiler::new()
            .compile_program(&parse(&src).expect("parse"))
            .expect("compile");
        assert!(
            chunk.ops.iter().any(
                |o| matches!(o, Op::HashSliceDerefIncDec(k, n) if *k == expected_kind && *n == 2)
            ),
            "expected HashSliceDerefIncDec({}, 2) for {:?}, got {:?}",
            expected_kind,
            tail,
            chunk.ops
        );
    }
}

#[test]
fn try_vm_execute_hash_slice_deref_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => undef }
        my $r = $h
        @$r{"a"} //= 42
        $r->{"a"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "single-key @$href{{\"k\"}} //= should compile (JumpIfDefinedKeep + SetArrowHashKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_hash_slice_deref_defined_or_assign_short_circuit() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "a" => 1 }
        my $r = $h
        my $runs = 0
        @$r{"a"} //= ($runs = 1)
        $runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "single-key @$href{{\"k\"}} //= should skip RHS when LHS is defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_hash_slice_deref_pre_inc_only() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "x" => 9 }
        my $r = $h
        ++@$r{"x"}
        $r->{"x"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 10);
}

#[test]
fn try_vm_execute_hash_slice_deref_pre_post_inc() {
    let p = parse(
        r#"no strict 'vars'
        my $h = { "x" => 9 }
        my $r = $h
        my $pre = ++@$r{"x"}
        my $post = @$r{"x"}++
        $pre + $post + $r->{"x"}"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "pre/post ++ on single-key @$href slice should compile (ArrowHash + ArrowHashPostfix)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 31);
}

/// `@$aref[i1,i2,...] = LIST` — multi-index array slice assignment goes through
/// [`Op::SetArrowArraySlice`] delegating to `Interpreter::assign_arrow_array_slice`.
#[test]
fn try_vm_execute_multi_index_array_slice_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [10, 20, 30, 40, 50]
        @$r[1, 3] = (200, 400)
        join(",", @$r)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-index array slice assign should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,200,30,400,50");
}

/// `@name[i1,i2,...] = LIST` — [`Op::SetNamedArraySlice`] element-wise like `@$aref[...]`.
#[test]
fn try_vm_execute_named_array_slice_assign_list() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (10, 20, 30, 40, 50)
        @a[1, 3] = (200, 400)
        join(",", @a)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named multi-index slice assign should compile (SetNamedArraySlice)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,200,30,400,50");
}

#[test]
fn try_vm_execute_named_array_slice_assign_range_subscript() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (0, 0, 0)
        @a[0..1] = (7, 8)
        $a[0] . "," . $a[1] . "," . $a[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "@a[0..1] = list should compile");
    assert_eq!(out.unwrap().expect("vm").to_string(), "7,8,0");
}

/// `@$aref[i1,i2,...] += rhs` — Perl 5 updates only the **last** index.
#[test]
fn try_vm_execute_multi_index_array_slice_compound_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [10, 20, 30]
        @$r[0, 2] += 5
        my $a = $r->[0]
        my $b_def = defined($r->[2]) ? 1 : 0
        $a * 10 + $b_def"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index compound assign should compile");
    assert_eq!(out.unwrap().expect("vm").to_int(), 101);
}

/// `++@$aref[i1,i2,i3]` multi-index — only the last element is incremented.
#[test]
fn try_vm_execute_multi_index_array_slice_pre_inc() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [100, 200, 300]
        my $pre = ++@$r[0, 1, 2]
        my $first = $r->[0]
        $pre * 10 + $first"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index pre-inc should compile");
    assert_eq!(out.unwrap().expect("vm").to_int(), 3110);
}

/// `@$aref[i1,i2]--` postfix — returns old **last** element; only that index is decremented.
#[test]
fn try_vm_execute_multi_index_array_slice_post_dec() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [100, 200]
        my $post = @$r[0, 1]--
        $post . ":" . $r->[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index postfix -- should compile");
    assert_eq!(out.unwrap().expect("vm").to_string(), "200:100");
}

/// `@$r[i,j] //=` — short-circuit uses the **last** slice element (Perl).
#[test]
fn try_vm_execute_multi_index_arrow_slice_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [1, 0]
        @$r[0,1] //= (5, 6)
        $r->[0] . "," . $r->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-index //= on array slice should compile to VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,0");
}

#[test]
fn try_vm_execute_multi_index_arrow_slice_defined_or_assign_fills_undef_last() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [1, undef]
        @$r[0, 1] //= (5, 6)
        $r->[0] . "," . $r->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some());
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,6");
}

/// `@$r[i,j] ||=` — only the last index is updated when the last slot is falsy.
#[test]
fn try_vm_execute_multi_index_arrow_slice_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [0, 0]
        @$r[0, 1] ||= (3, 4)
        $r->[0] . "," . $r->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index ||= should compile to VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "0,4");
}

/// `@$r[i,j] &&=` — only the last index is updated when the last slot is truthy.
#[test]
fn try_vm_execute_multi_index_arrow_slice_log_and_assign() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [1, 2]
        @$r[0, 1] &&= (0, 0)
        $r->[0] . "," . $r->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index &&= should compile to VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,0");
}

/// `++@a[i1,i2,...]` on a **named** array — same “last index only” rule as `@$aref[...]`.
#[test]
fn try_vm_execute_named_array_multi_slice_pre_inc() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (100, 200, 300)
        my $pre = ++@a[0, 1, 2]
        my $first = $a[0]
        $pre * 10 + $first"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named array multi-index pre-inc should compile (NamedArraySliceIncDec)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3110);
}

/// Range / list subscripts on a **named** array slice for `++` flatten like Perl (last index updates).
#[test]
fn try_vm_execute_named_array_slice_pre_inc_range_and_list_subscript() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (100, 200, 300)
        my $x = ++@a[0..1]
        my @b = (10, 20, 30)
        my $y = ++@b[(0, 1)]
        $x * 1000 + $y * 10 + $a[0] + $b[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named array slice pre-inc with range/list subscript should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 201_331);
}

/// `@a[i,j] //=` / `||=` / `&&=` — short-circuit tests **last** flattened slot only (named slice).
#[test]
fn try_vm_execute_named_array_multi_slice_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 0)
        @a[0, 1] //= (5, 6)
        $a[0] . "," . $a[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "named multi-index //= should compile to VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,0");
}

#[test]
fn try_vm_execute_named_array_multi_slice_log_or_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (0, 0)
        @a[0, 1] ||= (3, 4)
        $a[0] . "," . $a[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "named multi-index ||= should compile (NamedArraySlicePeekLast + SetNamedArraySliceLastKeep)"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "0,4");
}

#[test]
fn try_vm_execute_named_array_multi_slice_log_and_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (1, 2)
        @a[0, 1] &&= (0, 0)
        $a[0] . "," . $a[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "named multi-index &&= should compile to VM");
    assert_eq!(out.unwrap().expect("vm").to_string(), "1,0");
}

#[test]
fn try_vm_execute_named_array_multi_slice_plus_assign() {
    let p = parse(
        r#"no strict 'vars'
        my @a = (10, 20, 30)
        @a[0, 1, 2] += 7
        $a[0] + $a[1] + $a[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "named multi-index += should compile to VM");
    assert_eq!(out.unwrap().expect("vm").to_int(), 10 + 20 + 37);
}

/// `@$aref[range]` read uses flattened indices (not array length as index).
#[test]
fn try_vm_execute_arrow_array_slice_read_range_subscript() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [10, 20, 30]
        join(",", @$r[0..1])"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "@$r[0..1] read should compile with flattened slice specs"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,20");
}

/// `++@$r[0..1]` — same last-index rule as `@a[0..1]`.
#[test]
fn try_vm_execute_arrow_array_slice_pre_inc_range() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [100, 200, 300]
        my $pre = ++@$r[0..1]
        $pre . ":" . $r->[0] . ":" . $r->[1]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "++@$r[0..1] should compile");
    assert_eq!(out.unwrap().expect("vm").to_string(), "201:100:201");
}

/// `@$r[0..1] = (u,v)` element-wise assign through a range subscript.
#[test]
fn try_vm_execute_arrow_array_slice_assign_range_subscript() {
    let p = parse(
        r#"no strict 'vars'
        my $r = [0, 0, 0]
        @$r[0..1] = (7, 8)
        $r->[0] . "," . $r->[1] . "," . $r->[2]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "@$r[0..1] = list should compile");
    assert_eq!(out.unwrap().expect("vm").to_string(), "7,8,0");
}

/// `next` inside an `if` body (nested block) must jump to the enclosing loop's continue point.
/// Previously `last`/`next` only worked at the immediate loop-body level.
#[test]
fn try_vm_execute_next_nested_in_if() {
    let p = parse(
        r#"no strict 'vars'
        my $i = 0
        my $sum = 0
        while ($i < 5) {
            $i++
            if ($i == 3) {
                next
            }
            $sum += $i
        }
        $sum"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "nested next in if should compile");
    // 1 + 2 + 4 + 5 = 12
    assert_eq!(out.unwrap().expect("vm").to_int(), 12);
}

/// `last` inside a nested `if` inside a `while` body exits the loop, with `PopFrame` unwinding
/// the if-body scope frame.
#[test]
fn try_vm_execute_last_nested_in_if() {
    let p = parse(
        r#"no strict 'vars'
        my $i = 0
        my $sum = 0
        while ($i < 10) {
            $i++
            if ($i == 4) {
                last
            }
            $sum += $i
        }
        $sum * 10 + $i"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "nested last in if should compile");
    // sum = 1+2+3 = 6; i = 4 → 64
    assert_eq!(out.unwrap().expect("vm").to_int(), 64);
}

/// Labeled `last LABEL` from a deeply nested position — jumps through multiple scope frames
/// if needed.
#[test]
fn try_vm_execute_last_label_from_nested() {
    let p = parse(
        r#"no strict 'vars'
        my $hit = 0
        OUTER: while (1) {
            my $j = 0
            while ($j < 3) {
                $j++
                if ($j == 2) {
                    last OUTER
                }
                $hit++
            }
        }
        $hit"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "labeled last from nested while should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

/// `redo` jumps to the loop body head (skips `while` condition re-test for that iteration).
#[test]
fn try_vm_execute_redo_while_restarts_body() {
    let p = parse(
        r#"
        my $x = 0
        while ($x < 10) {
            $x++
            if ($x == 1) { redo; }
            last
        }
        $x
        "#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "redo in while should compile on VM path");
    assert_eq!(out.unwrap().expect("vm").to_int(), 2);
}

/// `use strict` + all vars declared: VM path compiles and runs (previously bailed to tree).
#[test]
fn try_vm_execute_strict_vars_happy_path() {
    let p = parse(
        r#"use strict
        use warnings
        my $x = 5
        my $y = 10
        $x + $y"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "use strict + declared vars should now compile through VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 15);
}

/// `use strict` + an undeclared scalar: compile-time rejection via CompileError::Frozen,
/// promoted to a user-visible error (not silently running through the tree fallback, which
/// had a pre-existing bug where scalar assignments bypass strict_vars).
#[test]
fn try_vm_execute_strict_vars_rejects_undeclared_scalar() {
    let p = parse(
        r#"use strict
        $undeclared = 5
        $undeclared"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "strict violation must be reported, not swallowed"
    );
    let err = out.unwrap().expect_err("should be an error");
    let s = err.to_string();
    assert!(
        s.contains("Global symbol \"$undeclared\"") && s.contains("explicit package name"),
        "unexpected error: {}",
        s
    );
}

/// `@_` is always bound in sub bodies and must be accessible under strict (exempt list).
#[test]
fn try_vm_execute_strict_vars_allows_underscore_and_foreach_var() {
    let p = parse(
        r#"use strict
        sub sum {
            my $s = 0
            for my $x (@_) {
                $s += $x
            }
            return $s
        }
        sum(1, 2, 3, 4, 5)"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "use strict with @_ and `for my $x` should compile through VM"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 15);
}

/// `use strict 'refs'` still fires through the transitive tree helpers the VM delegates into
/// (symbolic deref path). The VM no longer bails on strict_refs being set — we rely on the
/// shared `Interpreter::*` helpers to emit the error at runtime.
#[test]
fn try_vm_execute_strict_refs_via_transitive_helper() {
    let p = parse(
        r#"use strict 'refs'
        my $name = "foo"
        my @a = @$name
        $a[0]"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    // This one may either compile (and error at runtime via SymbolicDeref) or bail to tree.
    // Either way, the user must see an error mentioning "strict refs".
    let err_s = if let Some(r) = out {
        r.expect_err("expected error").to_string()
    } else {
        i.execute(&p).expect_err("expected error").to_string()
    };
    assert!(
        err_s.contains("strict refs"),
        "expected strict refs error, got: {}",
        err_s
    );
}

/// Regression: `my $s = 0; $s += 5;` inside a sub body must update the slot-bound lexical,
/// not a separately-named global. The pre-existing VM bug (name-based ScalarCompoundAssign on
/// a slot-based lexical) was masked by the strict-pragma bail; slice 6 fixes it directly by
/// emitting a slot-aware read-modify-write for compound assigns on slot variables.
#[test]
fn try_vm_execute_compound_assign_on_slot_lexical_in_sub() {
    let p = parse(
        r#"no strict 'vars'
        sub foo {
            my $s = 0
            $s += 5
            $s *= 2
            return $s
        }
        foo()"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "compound assign on sub-local lexical should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 10);
}

/// Scalar `.=` on a slot lexical must use concat-append lowering (`ConcatAppendSlot`), not
/// `GetScalarSlot` + `Concat` + `SetScalarSlot` (which clones the growing string each time).
#[test]
fn try_vm_execute_concat_compound_assign_on_slot_lexical_in_sub() {
    let p = parse(
        r#"no strict 'vars'
        sub foo {
            my $s = ""
            $s .= "ab"
            $s .= "cd"
            return $s
        }
        foo()"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "concat compound assign on sub-local lexical should compile"
    );
    assert_eq!(
        out.unwrap().expect("vm").to_string(),
        "abcd",
        "concat append must preserve slot binding and string contents"
    );
}

/// `goto LABEL` at the main-program top level: forward jump skips intermediate statements
/// and resumes at the labeled statement. VM path must resolve the label at compile time.
#[test]
fn try_vm_execute_top_level_goto_forward() {
    let p = parse(
        r#"no strict 'vars'
        my $x = 0
        goto END
        $x = 99
        END: $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "top-level `goto LABEL` should compile");
    assert_eq!(
        out.unwrap().expect("vm").to_int(),
        0,
        "goto should have skipped the `$x = 99;` assignment"
    );
}

/// `goto` inside a subroutine body resolves to a label in the same sub (separate scope from
/// main-program labels).
#[test]
fn try_vm_execute_sub_body_goto_forward() {
    let p = parse(
        r#"no strict 'vars'
        sub foo {
            my $r = 10
            goto SKIP
            $r = 20
            SKIP: return $r
        }
        foo()"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "sub-body `goto LABEL` should compile");
    assert_eq!(out.unwrap().expect("vm").to_int(), 10);
}

/// Backward `goto LABEL` (label defined before the goto): compiler must emit a backward jump
/// to the already-seen label IP. Guarded by a conditional wrap inside the label block so the
/// test actually terminates; the wrap uses a statement-level `if` (not postfix) because
/// postfix `if`/`unless` pushes a scope frame, which the VM goto implementation currently
/// does not cross (falls back to tree).
#[test]
fn try_vm_execute_goto_backward_unconditional_after_return() {
    // Simplest backward-goto shape: jump back to a label that has already been emitted.
    // To avoid an infinite loop we only execute the goto once, guarded by a counter.
    let p = parse(
        r#"no strict 'vars'
        my $sum = 0
        my $i = 0
        LOOP: $sum = $sum + $i
        $i = $i + 1
        if ($i < 4) { goto LOOP; }
        $sum"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    // The `if` wraps `goto` in a frame, so the VM's frame-crossing guard rejects this
    // case — the tree fallback takes over and errors with "goto outside goto-aware block".
    // Assert the VM path's behavior: either it compiles (if frame check is relaxed later)
    // or it bails to None and tree errors. For now, just assert we don't get a wrong result.
    if let Some(r) = out {
        assert_eq!(
            r.expect("vm").to_int(),
            6,
            "if it compiles, result must be 6"
        );
    } else {
        // VM frame-crossing bail is acceptable as of slice 4 scope.
        assert!(i.execute(&p).is_err());
    }
}

/// Narrower backward-goto case: both label and goto at the same (top-level) frame depth.
/// Uses a sentinel variable to terminate instead of a conditional wrap.
#[test]
fn try_vm_execute_goto_backward_same_frame() {
    // One-shot backward: LOOP sets a flag and unconditionally goto-skips via a second label.
    // This exercises the back-patch: LOOP is seen first, then `goto LOOP` resolves to a
    // backward jump. We only execute the backward jump zero times because the flag short-circuits
    // the path to it via a forward goto to DONE.
    let p = parse(
        r#"no strict 'vars'
        my $x = 0
        LOOP: $x = $x + 1
        goto DONE
        goto LOOP
        DONE: $x"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "same-frame backward goto (in a never-executed position) should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
}

/// `goto` to an unknown label errors out (CompileError::Frozen → try_vm_execute returns None,
/// and the tree fallback then produces its own "goto outside goto-aware block" error; we just
/// assert that running the program fails somehow).
#[test]
fn try_vm_execute_goto_unknown_label_errors() {
    let p = parse(
        r#"no strict 'vars'
        goto NO_SUCH"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    // Either VM returns Some(Err) (if compile-error promotion is wired) or None→tree errors.
    let out = try_vm_execute(&p, &mut i);
    if let Some(r) = out {
        assert!(r.is_err(), "goto to unknown label should error");
    } else {
        assert!(i.execute(&p).is_err(), "tree fallback should also error");
    }
}

/// `while (COND) { BODY } continue { POST }` — the continue block runs after every iteration
/// of BODY, on normal fall-through. VM path must emit the continue block before the jump back
/// to the condition test.
#[test]
fn try_vm_execute_while_with_continue_block() {
    let p = parse(
        r#"no strict 'vars'
        my $sum = 0
        my $i = 0
        while ($i < 5) {
            $sum += $i
        } continue {
            $i++
        }
        # sum = 0+1+2+3+4 = 10; i = 5
        $sum * 10 + $i"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "while + continue block should compile");
    assert_eq!(out.unwrap().expect("vm").to_int(), 105);
}

/// `foreach ... continue { ... }` runs the continue block after each body iteration, before
/// advancing the iterator. Keeping body side-effect free to avoid the `last/next` nested-block
/// bailout (tested separately with a top-level `next`).
#[test]
fn try_vm_execute_foreach_with_continue_block() {
    let p = parse(
        r#"no strict 'vars'
        my $body = 0
        my $cont = 0
        foreach my $x (1..4) {
            $body += $x
        } continue {
            $cont += $x
        }
        # body = 1+2+3+4 = 10; cont = 10
        $body * 100 + $cont"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "foreach + continue block should compile");
    assert_eq!(out.unwrap().expect("vm").to_int(), 1010);
}

/// `next` at top level of a while body (not nested in `if`) routes through the continue block.
#[test]
fn try_vm_execute_while_continue_with_top_level_next() {
    let p = parse(
        r#"no strict 'vars'
        my $cont_runs = 0
        my $i = 0
        while ($i < 3) {
            $i++
            next
        } continue {
            $cont_runs++
        }
        # cont_runs should be 3 (continue runs per iteration even when next fires)
        $cont_runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "while + continue block with top-level `next` should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

/// `until (COND) { BODY } continue { POST }` — same semantics as while, inverted condition.
#[test]
fn try_vm_execute_until_with_continue_block() {
    let p = parse(
        r#"no strict 'vars'
        my $i = 0
        my $cont_runs = 0
        until ($i >= 3) {
            # body does nothing except guard
        } continue {
            $i++
            $cont_runs++
        }
        $i * 10 + $cont_runs"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "until + continue block should compile");
    // i ends at 3, cont_runs = 3
    assert_eq!(out.unwrap().expect("vm").to_int(), 33);
}

/// `++@{…}` / `%{…}++` are rejected at compile time by the VM path — [`try_vm_execute`] returns
/// `Some(Err(_))` (not `None`), so the fallback to the tree interpreter is no longer needed.
/// Error message matches the tree-walker's `Can't modify {array,hash} dereference in …`.
#[test]
fn try_vm_execute_rejects_aggregate_symbolic_inc_dec_directly() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    // VM path returns Some(Err(_)) — i.e. the compiler emitted the error op, not Unsupported.
    let cases: &[(&str, &str, &str)] = &[
        (
            "no strict 'vars'; my $r = [1,2]; ++@$r;",
            "array dereference",
            "preincrement",
        ),
        (
            "no strict 'vars'; my $r = [1,2]; --@$r;",
            "array dereference",
            "predecrement",
        ),
        (
            "no strict 'vars'; my $r = [1,2]; @$r++;",
            "array dereference",
            "postincrement",
        ),
        (
            "no strict 'vars'; my $hr = {a=>1}; %$hr--;",
            "hash dereference",
            "postdecrement",
        ),
    ];
    for (src, want_agg, want_op) in cases {
        let p = parse(src).expect("parse");
        let mut i = Interpreter::new();
        let out = try_vm_execute(&p, &mut i);
        assert!(
            out.is_some(),
            "VM path must reject {:?} directly (Some(Err(_))), not fall back to tree",
            src
        );
        let err = out.unwrap().expect_err("VM should error");
        let s = err.to_string();
        assert!(
            s.contains(want_agg) && s.contains(want_op),
            "unexpected error for {:?}: {}",
            src,
            s
        );
        // And compile-shape: the chunk contains the RuntimeErrorConst op.
        let chunk = Compiler::new()
            .compile_program(&parse(src).expect("parse"))
            .expect("compile should not error for this shape");
        assert!(
            chunk
                .ops
                .iter()
                .any(|o| matches!(o, Op::RuntimeErrorConst(_))),
            "expected Op::RuntimeErrorConst in chunk for {:?}, got {:?}",
            src,
            chunk.ops
        );
    }
}

/// Perl 5 rejects `++@{...}`, `%{...}++`, etc.; we must not treat them as numeric ops on length.
#[test]
fn symbolic_array_hash_deref_inc_dec_errors_like_perl() {
    let mut i = Interpreter::new();
    let p = parse(
        r#"no strict 'vars'
        my $r = [1, 2, 3]
        ++@{ $r }"#,
    )
    .expect("parse");
    let e = i.execute(&p).expect_err("++@{...} is invalid in Perl 5");
    let s = e.to_string();
    assert!(
        s.contains("array dereference") && s.contains("preincrement"),
        "{s}"
    );

    let p2 = parse(
        r#"no strict 'vars'
        my $h = { a => 1 }
        my $hr = $h
        %{ $hr }++"#,
    )
    .expect("parse");
    let e2 = i.execute(&p2).expect_err("%{...}++ is invalid in Perl 5");
    let s2 = e2.to_string();
    assert!(
        s2.contains("hash dereference") && s2.contains("postincrement"),
        "{s2}"
    );

    let p3 = parse(
        r#"no strict 'vars'
        my $r = [1]
        --@$r"#,
    )
    .expect("parse");
    let e3 = i.execute(&p3).expect_err("--@$r is invalid in Perl 5");
    let s3 = e3.to_string();
    assert!(
        s3.contains("array dereference") && s3.contains("predecrement"),
        "{s3}"
    );
}

#[test]
fn try_vm_execute_grep_expr_comma() {
    let p = parse(
        r#"no strict 'vars'
        join(",", grep $_ > 1, (1, 2, 3))"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "grep EXPR, LIST should compile (Op::GrepWithExpr), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "2,3");
}

#[test]
fn try_vm_execute_map_expr_comma() {
    let p = parse(
        r#"no strict 'vars'
        join(",", map $_ * 2, (1, 2, 3))"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "map EXPR, LIST should compile (Op::MapWithExpr), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "2,4,6");
}

#[test]
fn try_vm_execute_runs_begin_block_before_main() {
    let p = parse("BEGIN { 1; } 2").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 2);
}

#[test]
fn lint_program_accepts_vm_compilable_program() {
    let p = parse("42").expect("parse");
    let mut i = Interpreter::new();
    assert!(lint_program(&p, &mut i).is_ok());
}

#[test]
fn bench_builtin_reports_stats() {
    let v = run("bench { 1 + 1 } 5").expect("run");
    let s = v.to_string();
    assert!(s.contains("bench:"));
    assert!(s.contains("min="));
    assert!(s.contains("p99="));
}

#[test]
fn try_vm_execute_runs_given_when_and_algebraic_match() {
    let p_given = parse(r#"given (7) { when (7) { 99; } default { -1; } }"#).expect("parse given");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p_given, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 99);

    let p_match = parse(r#"match (2) { _ => 3 + 4, }"#).expect("parse match");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p_match, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 7);
}

#[test]
fn run_empty_statement_list_undef_or_zero() {
    let v = run(";;").expect("run");
    assert!(v.is_undef() || v.to_int() == 0);
}

#[test]
fn parse_returns_empty_program_for_whitespace() {
    let p = parse("   \n  ").expect("parse");
    assert!(p.statements.is_empty());
}

#[test]
fn run_builtin_abs_int_sqrt() {
    assert_eq!(run_int("abs(-9)"), 9);
    assert_eq!(run_int("int(9.9)"), 9);
    assert_eq!(run_int("sqrt(49)"), 7);
}

#[test]
fn run_length_uc_lc() {
    assert_eq!(run_int(r#"length("abc")"#), 3);
    assert_eq!(run(r#"uc("ab")"#).expect("run").to_string(), "AB");
    assert_eq!(run(r#"lc("CD")"#).expect("run").to_string(), "cd");
}

#[test]
fn run_array_push_pop_shift() {
    assert_eq!(run_int("my @a = (1, 2); push @a, 3; scalar @a"), 3);
    assert_eq!(run_int("my @b = (7, 8, 9); pop @b"), 9);
    assert_eq!(run_int("my @c = (7, 8, 9); shift @c"), 7);
}

#[test]
fn run_join_reverse_sort_numbers() {
    assert_eq!(
        run(r#"join("-", 1, 2, 3)"#).expect("run").to_string(),
        "1-2-3"
    );
    assert_eq!(
        run(r#"scalar reverse (1, 2, 3)"#).expect("run").to_string(),
        "321"
    );
}

#[test]
fn run_hash_keys_values() {
    assert_eq!(run_int(r#"my %h = (a => 1, b => 2); scalar keys %h"#), 2);
}

#[test]
fn run_ord_chr_roundtrip() {
    assert_eq!(run_int(r#"ord("A")"#), 65);
    assert_eq!(run(r#"chr(65)"#).expect("run").to_string(), "A");
}

#[test]
fn run_defined_and_undef_scalar() {
    assert_eq!(run_int(r#"defined("x")"#), 1);
    assert_eq!(run_int(r#"defined(undef)"#), 0);
}

#[test]
fn run_string_compare_str_ops() {
    assert_eq!(run_int(r#""a" lt "b""#), 1);
    assert_eq!(run_int(r#""b" gt "a""#), 1);
    assert_eq!(run_int(r#""a" le "a""#), 1);
    assert_eq!(run_int(r#""b" ge "b""#), 1);
}

#[test]
fn run_do_block_value() {
    assert_eq!(run_int("do { 6 * 7 }"), 42);
}

#[test]
fn run_foreach_accumulator() {
    assert_eq!(
        run_int("my $s = 0; foreach my $n (1, 2, 3, 4) { $s = $s + $n; } $s"),
        10
    );
}

#[test]
fn run_while_counter() {
    assert_eq!(run_int("my $i = 0; while ($i < 5) { $i = $i + 1; } $i"), 5);
}
