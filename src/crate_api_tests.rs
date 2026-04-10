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
fn run_compound_assign_xor_shift() {
    assert_eq!(run_int(r#"my $x = 4; $x <<= 2; $x;"#), 16);
    assert_eq!(run_int(r#"my $x = 16; $x >>= 2; $x;"#), 4);
    assert_eq!(run_int(r#"my $x = 5; $x ^= 3; $x;"#), 6);
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
fn run_log_and_compound_assign() {
    assert_eq!(run_int("my $x = 0; $x &&= 5; $x;"), 0);
    assert_eq!(run_int("my $y = 2; $y &&= 7; $y;"), 7);
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
fn try_vm_execute_scalar_defined_or_assign() {
    let p = parse(
        r#"my $x;
        $x //= 99;
        $x;"#,
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
        r#"my $x = 0;
        my $runs = 0;
        $x //= ($runs = 1);
        $runs;"#,
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
        r#"my $x = 0;
        $x ||= 8;
        $x;"#,
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
        r#"my $x = 5;
        my $runs = 0;
        $x ||= ($runs = 1);
        $runs;"#,
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
        r#"my $x = 2;
        $x &&= 7;
        $x;"#,
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
        r#"my $x = 0;
        my $runs = 0;
        $x &&= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "scalar &&= should skip RHS when LHS is false");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_array_elem_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my @a;
        $a[0] //= 11;
        $a[0];"#,
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
        r#"no strict 'vars';
        my @a = (9);
        my $runs = 0;
        $a[0] //= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "array element //= should skip RHS when LHS is defined");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_array_elem_log_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my @a = (0);
        $a[0] ||= 6;
        $a[0];"#,
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
        r#"no strict 'vars';
        my %h;
        $h{"x"} //= 33;
        $h{"x"};"#,
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
        r#"no strict 'vars';
        my %h = ("x" => 1);
        my $runs = 0;
        $h{"x"} //= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "hash element //= should skip RHS when LHS is defined");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_hash_elem_log_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my %h = ("x" => 0);
        $h{"x"} ||= 4;
        $h{"x"};"#,
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
        r#"no strict 'vars';
        my @a = (1);
        $a[0] &&= 8;
        $a[0];"#,
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
        r#"no strict 'vars';
        my %h = ("x" => 0);
        my $runs = 0;
        $h{"x"} &&= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "hash element &&= should skip RHS when LHS is false");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_log_and_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 1 };
        $h->{"a"} &&= 9;
        $h->{"a"};"#,
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
fn try_vm_execute_symbolic_scalar_deref() {
    let p = parse(
        r#"no strict 'vars';
        my $x = 42;
        my $r = \$x;
        $$r;"#,
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
        r#"no strict 'vars';
        my $x = 0;
        my $r = \$x;
        $$r = 7;
        $x;"#,
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
        r#"no strict 'vars';
        my $x = 10;
        my $r = \$x;
        $$r += 2;
        $x;"#,
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
        r#"no strict 'vars';
        my $x;
        my $r = \$x;
        $$r //= 99;
        $x;"#,
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
        r#"no strict 'vars';
        my $x = 0;
        my $r = \$x;
        my $runs = 0;
        $$r //= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$$r //= should skip RHS when LHS is defined"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_symbolic_scalar_ref_log_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $x = 0;
        my $r = \$x;
        $$r ||= 8;
        $x;"#,
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
        r#"no strict 'vars';
        my $x = 5;
        my $r = \$x;
        my $runs = 0;
        $$r ||= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "$$r ||= should skip RHS when LHS is true"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_compound_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 10 };
        $h->{"a"} += 2;
        $h->{"a"};"#,
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
        r#"no strict 'vars';
        my $h = { "a" => undef };
        $h->{"a"} //= 42;
        $h->{"a"};"#,
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
        r#"no strict 'vars';
        my $h = { "a" => 1 };
        my $runs = 0;
        $h->{"a"} //= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "arrow hash //= should skip RHS when LHS is defined");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_hash_log_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 0 };
        $h->{"a"} ||= 9;
        $h->{"a"};"#,
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
        r#"no strict 'vars';
        my $h = { "a" => 2 };
        my $runs = 0;
        $h->{"a"} ||= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "arrow hash ||= should skip RHS when LHS is true");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
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
fn try_vm_execute_arrow_array_compound_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $a = [10, 20];
        $a->[0] += 2;
        $a->[0];"#,
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
        r#"no strict 'vars';
        my $a = [undef];
        $a->[0] //= 7;
        $a->[0];"#,
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
        r#"no strict 'vars';
        my $a = [99];
        my $runs = 0;
        $a->[0] //= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "arrow array //= should skip RHS when LHS is defined");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_array_log_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $a = [0];
        $a->[0] ||= 5;
        $a->[0];"#,
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
        r#"no strict 'vars';
        my $a = [3];
        my $runs = 0;
        $a->[0] ||= ($runs = 1);
        $runs;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "arrow array ||= should skip RHS when LHS is true");
    assert_eq!(out.unwrap().expect("vm").to_int(), 0);
}

#[test]
fn try_vm_execute_arrow_array_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $a = [1, 2];
        $a->[2] = 3;
        $a->[0] + $a->[1] + $a->[2];"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "arrow array assign should compile (Op::SetArrowArray), not force tree fallback"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 6);
}

#[test]
fn try_vm_execute_arrow_array_pre_inc_only() {
    let p = parse(
        r#"no strict 'vars';
        my $a = [9];
        ++$a->[0];
        $a->[0];"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 10);
}

#[test]
fn try_vm_execute_arrow_hash_pre_inc_only() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "x" => 9 };
        ++$h->{"x"};
        $h->{"x"};"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 10);
}

#[test]
fn try_vm_execute_arrow_array_hash_pre_post_inc() {
    let p = parse(
        r#"no strict 'vars';
        my $a = [9];
        my $h = { "x" => 9 };
        my $pre_a = ++$a->[0];
        my $post_a = $a->[0]++;
        my $pre_h = ++$h->{"x"};
        my $post_h = $h->{"x"}++;
        $pre_a + $post_a + $a->[0] + $pre_h + $post_h + $h->{"x"};"#,
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
        r#"no strict 'vars';
        my $x = 9;
        my $r = \$x;
        my $pre = ++$$r;
        my $post = $$r++;
        $pre + $post + $x;"#,
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
        r#"no strict 'vars';
        my $a = [1, 2];
        my $r = $a;
        @{ $r } = (3, 4, 5);
        my $h = { "a" => 1 };
        my $hr = $h;
        %{ $hr } = ("b", 2, "c", 3);
        join(",", @$r) . ";" . join(",", sort keys %$hr);"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "symbolic array/hash deref assign should compile (SetSymbolicArrayRef / SetSymbolicHashRef)"
    );
    assert_eq!(
        out.unwrap().expect("vm").to_string(),
        "3,4,5;b,c"
    );
}

#[test]
fn try_vm_execute_hash_slice_deref() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { a => 10, b => 20 };
        my $r = $h;
        join(",", @$r{"a", "b"});"#,
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
        r#"no strict 'vars';
        my $h = { "a" => 1, "b" => 2 };
        my $r = $h;
        @$r{"a", "b"} = (10, 20);
        $r->{"a"} . "," . $r->{"b"};"#,
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

#[test]
fn try_vm_execute_hash_slice_deref_compound_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 10 };
        my $r = $h;
        @$r{"a"} += 2;
        $r->{"a"};"#,
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

#[test]
fn try_vm_execute_hash_slice_deref_defined_or_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => undef };
        my $r = $h;
        @$r{"a"} //= 42;
        $r->{"a"};"#,
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
        r#"no strict 'vars';
        my $h = { "a" => 1 };
        my $r = $h;
        my $runs = 0;
        @$r{"a"} //= ($runs = 1);
        $runs;"#,
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

/// Perl 5 rejects `++@{...}`, `%{...}++`, etc.; we must not treat them as numeric ops on length.
#[test]
fn symbolic_array_hash_deref_inc_dec_errors_like_perl() {
    let mut i = Interpreter::new();
    let p = parse(
        r#"no strict 'vars';
        my $r = [1, 2, 3];
        ++@{ $r };"#,
    )
    .expect("parse");
    let e = i.execute(&p).expect_err("++@{...} is invalid in Perl 5");
    let s = e.to_string();
    assert!(
        s.contains("array dereference") && s.contains("preincrement"),
        "{s}"
    );

    let p2 = parse(
        r#"no strict 'vars';
        my $h = { a => 1 };
        my $hr = $h;
        %{ $hr }++;"#,
    )
    .expect("parse");
    let e2 = i.execute(&p2).expect_err("%{...}++ is invalid in Perl 5");
    let s2 = e2.to_string();
    assert!(
        s2.contains("hash dereference") && s2.contains("postincrement"),
        "{s2}"
    );

    let p3 = parse(
        r#"no strict 'vars';
        my $r = [1];
        --@$r;"#,
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
