//! Unit tests for the crate root API: `parse`, `run`, `parse_and_run_string`, `try_vm_execute`.

use crate::interpreter::Interpreter;
use crate::{lint_program, parse, parse_and_run_string, parse_with_file, run, try_vm_execute};

fn run_int(code: &str) -> i64 {
    run(code).expect("run").to_int()
}

#[test]
fn run_arithmetic_add_sub_mul_div_mod() {
    assert_eq!(run_int("11 + 4;"), 15);
    assert_eq!(run_int("20 - 7;"), 13);
    assert_eq!(run_int("6 * 9;"), 54);
    assert_eq!(run_int("22 / 4;"), 5);
    assert_eq!(run_int("17 % 5;"), 2);
}

#[test]
fn run_power_and_precedence() {
    assert_eq!(run_int("2 ** 8;"), 256);
    assert_eq!(run_int("2 + 3 * 4;"), 14);
    assert_eq!(run_int("(2 + 3) * 4;"), 20);
}

#[test]
fn run_numeric_comparisons_yield_perl_truth() {
    assert_eq!(run_int("5 == 5;"), 1);
    assert_eq!(run_int("5 != 3;"), 1);
    assert_eq!(run_int("3 < 5;"), 1);
    assert_eq!(run_int("5 > 3;"), 1);
    assert_eq!(run_int("5 <= 5;"), 1);
    assert_eq!(run_int("5 >= 4;"), 1);
}

#[test]
fn run_spaceship_operator() {
    assert_eq!(run_int("5 <=> 3;"), 1);
    assert_eq!(run_int("3 <=> 5;"), -1);
    assert_eq!(run_int("4 <=> 4;"), 0);
}

#[test]
fn run_string_cmp_and_eq() {
    assert_eq!(run_int(r#""a" cmp "b";"#), -1);
    assert_eq!(run_int(r#""b" cmp "a";"#), 1);
    assert_eq!(run_int(r#""a" eq "a";"#), 1);
    assert_eq!(run_int(r#""a" ne "b";"#), 1);
}

#[test]
fn run_logical_short_circuit() {
    assert_eq!(run_int("1 && 7;"), 7);
    assert_eq!(run_int("0 && 7;"), 0);
    assert_eq!(run_int("0 || 8;"), 8);
    assert_eq!(run_int("3 || 8;"), 3);
}

#[test]
fn run_defined_or_operator() {
    assert_eq!(run_int("undef // 99;"), 99);
    assert_eq!(run_int("0 // 5;"), 0);
}

#[test]
fn run_bitwise_ops() {
    assert_eq!(run_int("0x0F & 0x33;"), 0x03);
    assert_eq!(run_int("0x01 | 0x02;"), 0x03);
    assert_eq!(run_int("0x0F ^ 0x33;"), 0x3C);
}

#[test]
fn run_unary_minus_and_not() {
    assert_eq!(run_int("- 42;"), -42);
    assert_eq!(run_int("!0;"), 1);
    assert_eq!(run_int("!1;"), 0);
}

#[test]
fn run_concat_and_repeat() {
    assert_eq!(run(r#""a" . "b" . "c";"#).expect("run").to_string(), "abc");
    assert_eq!(run(r#""x" x 4;"#).expect("run").to_string(), "xxxx");
}

#[test]
fn run_list_and_scalar_context_array() {
    assert_eq!(run_int("scalar (1, 2, 3);"), 3);
}

#[test]
fn run_my_variable_and_assignment() {
    assert_eq!(run_int("my $x = 41; $x + 1;"), 42);
}

#[test]
fn run_conditional_expression() {
    assert_eq!(run_int("1 ? 10 : 20;"), 10);
    assert_eq!(run_int("0 ? 10 : 20;"), 20);
}

#[test]
fn run_simple_subroutine() {
    assert_eq!(
        run_int("sub add2 { return $_[0] + $_[1]; } add2(30, 12);"),
        42
    );
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
    let p = parse("42;").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some());
    assert_eq!(out.unwrap().expect("vm").to_int(), 42);
}

#[test]
fn try_vm_execute_indirect_coderef_call() {
    let p = parse("my $inc = sub { $_[0] + 1 }; $inc(41);").expect("parse");
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
        r#"no strict 'vars';
        my $cmp = sub { $a <=> $b };
        join(",", sort $cmp (3, 1, 2));"#,
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
fn try_vm_execute_arrow_hash_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 1 };
        $h->{"b"} = 2;
        $h->{"a"} + $h->{"b"};"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow hash assign should compile (Op::SetArrowHash), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 3);
}

#[test]
fn try_vm_execute_grep_expr_comma() {
    let p = parse(
        r#"no strict 'vars';
        join(",", grep $_ > 1, (1, 2, 3));"#,
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
fn try_vm_execute_runs_begin_block_before_main() {
    let p = parse("BEGIN { 1; } 2;").expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 2);
}

#[test]
fn lint_program_accepts_vm_compilable_program() {
    let p = parse("42;").expect("parse");
    let mut i = Interpreter::new();
    assert!(lint_program(&p, &mut i).is_ok());
}

#[test]
fn bench_builtin_reports_stats() {
    let v = run("bench { 1 + 1 } 5;").expect("run");
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
    let v = run(";;;").expect("run");
    assert!(v.is_undef() || v.to_int() == 0);
}

#[test]
fn parse_returns_empty_program_for_whitespace() {
    let p = parse("   \n  ").expect("parse");
    assert!(p.statements.is_empty());
}

#[test]
fn run_builtin_abs_int_sqrt() {
    assert_eq!(run_int("abs(-9);"), 9);
    assert_eq!(run_int("int(9.9);"), 9);
    assert_eq!(run_int("sqrt(49);"), 7);
}

#[test]
fn run_length_uc_lc() {
    assert_eq!(run_int(r#"length("abc");"#), 3);
    assert_eq!(run(r#"uc("ab");"#).expect("run").to_string(), "AB");
    assert_eq!(run(r#"lc("CD");"#).expect("run").to_string(), "cd");
}

#[test]
fn run_array_push_pop_shift() {
    assert_eq!(run_int("my @a = (1, 2); push @a, 3; scalar @a;"), 3);
    assert_eq!(run_int("my @b = (7, 8, 9); pop @b;"), 9);
    assert_eq!(run_int("my @c = (7, 8, 9); shift @c;"), 7);
}

#[test]
fn run_join_reverse_sort_numbers() {
    assert_eq!(
        run(r#"join("-", 1, 2, 3);"#).expect("run").to_string(),
        "1-2-3"
    );
    assert_eq!(run_int("reverse (1, 2, 3);"), 3);
}

#[test]
fn run_hash_keys_values() {
    assert_eq!(run_int(r#"my %h = (a => 1, b => 2); scalar keys %h;"#), 2);
}

#[test]
fn run_ord_chr_roundtrip() {
    assert_eq!(run_int(r#"ord("A");"#), 65);
    assert_eq!(run(r#"chr(65);"#).expect("run").to_string(), "A");
}

#[test]
fn run_defined_and_undef_scalar() {
    assert_eq!(run_int(r#"defined("x");"#), 1);
    assert_eq!(run_int(r#"defined(undef);"#), 0);
}

#[test]
fn run_string_compare_str_ops() {
    assert_eq!(run_int(r#""a" lt "b";"#), 1);
    assert_eq!(run_int(r#""b" gt "a";"#), 1);
    assert_eq!(run_int(r#""a" le "a";"#), 1);
    assert_eq!(run_int(r#""b" ge "b";"#), 1);
}

#[test]
fn run_do_block_value() {
    assert_eq!(run_int("do { 6 * 7 };"), 42);
}

#[test]
fn run_foreach_accumulator() {
    assert_eq!(
        run_int("my $s = 0; foreach my $n (1, 2, 3, 4) { $s = $s + $n; } $s;"),
        10
    );
}

#[test]
fn run_while_counter() {
    assert_eq!(run_int("my $i = 0; while ($i < 5) { $i = $i + 1; } $i;"), 5);
}
