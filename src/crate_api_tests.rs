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
    assert!(
        out.is_some(),
        "scalar &&= should skip RHS when LHS is false"
    );
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
    assert!(
        out.is_some(),
        "array element //= should skip RHS when LHS is defined"
    );
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
    assert!(
        out.is_some(),
        "hash element //= should skip RHS when LHS is defined"
    );
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
    assert!(
        out.is_some(),
        "hash element &&= should skip RHS when LHS is false"
    );
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
    assert!(out.is_some(), "$$r //= should skip RHS when LHS is defined");
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
    assert!(out.is_some(), "$$r ||= should skip RHS when LHS is true");
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
    assert!(
        out.is_some(),
        "arrow hash //= should skip RHS when LHS is defined"
    );
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
    assert!(
        out.is_some(),
        "arrow hash ||= should skip RHS when LHS is true"
    );
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
    assert!(
        out.is_some(),
        "arrow array //= should skip RHS when LHS is defined"
    );
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
    assert!(
        out.is_some(),
        "arrow array ||= should skip RHS when LHS is true"
    );
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
fn try_vm_execute_arrow_array_slice_through_atref_read() {
    let p = parse(
        r#"no strict 'vars';
        my $a = [10, 20, 30];
        my $r = $a;
        join(",", @$r[1,2]);"#,
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
        r#"no strict 'vars';
        my $a = [10, 20];
        my $r = $a;
        @$r[1];"#,
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
        r#"no strict 'vars';
        my $a = [0, 0, 0, 0];
        my $r = $a;
        @$r[1,2] = (7, 8);
        $r->[1] . "," . $r->[2];"#,
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
    assert_eq!(out.unwrap().expect("vm").to_string(), "3,4,5;b,c");
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

/// Multi-key `@$href{k1,k2} += EXPR` goes through [`Op::HashSliceDerefCompound`] and must match the
/// tree-walker `CompoundAssign` generic fallback: read slice as list, fold via `eval_binop` (scalar
/// context — `@slice + 5 = length + 5`), then re-assign via `assign_hash_slice_deref` (first slot
/// gets the new scalar, rest become undef). This tracks tree semantics — Perl 5's per-last-element
/// `+=` on slices is a separate parity divergence noted in PARITY_ROADMAP Phase 2.
#[test]
fn try_vm_execute_hash_slice_deref_compound_assign_multi_key() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 10, "b" => 20 };
        my $r = $h;
        @$r{"a","b"} += 5;
        # length(2) + 5 = 7 goes into first slot; second becomes undef
        my $first = $r->{"a"};
        my $second = defined($r->{"b"}) ? 1 : 0;
        $first * 10 + $second;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "multi-key @$href{{k1,k2}} += should compile (HashSliceDerefCompound)"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 70);
}

/// Ensure the multi-key compound assign emits [`Op::HashSliceDerefCompound`], not a tree fallback.
#[test]
fn compile_hash_slice_deref_multi_key_compound_emits_dedicated_op() {
    use crate::bytecode::Op;
    use crate::compiler::Compiler;
    let chunk = Compiler::new()
        .compile_program(&parse("my %h = (); my $r = \\%h; @$r{\"a\",\"b\"} += 1;").expect("parse"))
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

/// `++@$href{k1,k2}` on a multi-key slice: uses [`Op::HashSliceDerefIncDec`] (kind=0),
/// matches tree-walker: new scalar = list length + 1, first slot = scalar, rest = undef.
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_pre_inc() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 10, "b" => 20, "c" => 30 };
        my $r = $h;
        my $pre = ++@$r{"a","b","c"};
        # list length 3 + 1 = 4 goes into first slot
        my $first = $r->{"a"};
        my $second_def = defined($r->{"b"}) ? 1 : 0;
        $pre * 100 + $first * 10 + $second_def;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "++ on multi-key @$href{{k1,k2,k3}} should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 440);
}

/// `@$href{k1,k2}++` (postfix) returns the old slice list; this test checks the list is
/// what tree-walker would return (old values) and that the first slot holds length+1 afterwards.
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_post_inc() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 10, "b" => 20, "c" => 30 };
        my $r = $h;
        my $post = @$r{"a","b","c"}++;
        # post is the old list (10, 20, 30); stringifying concatenates: "102030"
        # first slot now holds length(3) + 1 = 4
        my $first = $r->{"a"};
        my $second_def = defined($r->{"b"}) ? 1 : 0;
        # Combine into a single int to match both tree-walker and VM.
        $post . ":" . $first . ":" . $second_def;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "postfix ++ on multi-key @$href{{k1,k2,k3}} should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_string(), "102030:4:0");
}

/// Multi-key `--` (postfix): same pattern, subtracts 1.
#[test]
fn try_vm_execute_hash_slice_deref_multi_key_post_dec() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "a" => 100, "b" => 200 };
        my $r = $h;
        @$r{"a","b"}--;
        # length 2 - 1 = 1 → first slot; b becomes undef
        $r->{"a"};"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(
        out.is_some(),
        "postfix -- on multi-key slice should compile"
    );
    assert_eq!(out.unwrap().expect("vm").to_int(), 1);
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

#[test]
fn try_vm_execute_hash_slice_deref_pre_inc_only() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "x" => 9 };
        my $r = $h;
        ++@$r{"x"};
        $r->{"x"};"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i).expect("vm path");
    assert_eq!(out.expect("vm").to_int(), 10);
}

#[test]
fn try_vm_execute_hash_slice_deref_pre_post_inc() {
    let p = parse(
        r#"no strict 'vars';
        my $h = { "x" => 9 };
        my $r = $h;
        my $pre = ++@$r{"x"};
        my $post = @$r{"x"}++;
        $pre + $post + $r->{"x"};"#,
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
        r#"no strict 'vars';
        my $r = [10, 20, 30, 40, 50];
        @$r[1, 3] = (200, 400);
        join(",", @$r);"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index array slice assign should compile");
    assert_eq!(out.unwrap().expect("vm").to_string(), "10,200,30,400,50");
}

/// `@$aref[i1,i2,...] += rhs` — matches tree-walker's generic CompoundAssign fallback
/// (scalar-context on the slice via eval_binop, then element-wise re-assign).
#[test]
fn try_vm_execute_multi_index_array_slice_compound_assign() {
    let p = parse(
        r#"no strict 'vars';
        my $r = [10, 20, 30];
        @$r[0, 2] += 5;
        # length(2) + 5 = 7 assigned to slice → $r->[0] = 7, $r->[2] = undef
        my $a = $r->[0];
        my $b_def = defined($r->[2]) ? 1 : 0;
        $a * 10 + $b_def;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index compound assign should compile");
    assert_eq!(out.unwrap().expect("vm").to_int(), 70);
}

/// `++@$aref[i1,i2,i3]` multi-index — length+1 in first slot, rest undef.
#[test]
fn try_vm_execute_multi_index_array_slice_pre_inc() {
    let p = parse(
        r#"no strict 'vars';
        my $r = [100, 200, 300];
        my $pre = ++@$r[0, 1, 2];
        my $first = $r->[0];
        $pre * 10 + $first;"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index pre-inc should compile");
    // pre = length(3)+1 = 4; first slot = 4 → 4*10+4 = 44
    assert_eq!(out.unwrap().expect("vm").to_int(), 44);
}

/// `@$aref[i1,i2]--` postfix — returns old slice list.
#[test]
fn try_vm_execute_multi_index_array_slice_post_dec() {
    let p = parse(
        r#"no strict 'vars';
        my $r = [100, 200];
        my $post = @$r[0, 1]--;
        $post . ":" . $r->[0];"#,
    )
    .expect("parse");
    let mut i = Interpreter::new();
    let out = try_vm_execute(&p, &mut i);
    assert!(out.is_some(), "multi-index postfix -- should compile");
    // old list = (100, 200) stringifies to "100200"; first slot = length(2)-1 = 1
    assert_eq!(out.unwrap().expect("vm").to_string(), "100200:1");
}

/// `next` inside an `if` body (nested block) must jump to the enclosing loop's continue point.
/// Previously `last`/`next` only worked at the immediate loop-body level.
#[test]
fn try_vm_execute_next_nested_in_if() {
    let p = parse(
        r#"no strict 'vars';
        my $i = 0;
        my $sum = 0;
        while ($i < 5) {
            $i++;
            if ($i == 3) {
                next;
            }
            $sum += $i;
        }
        $sum;"#,
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
        r#"no strict 'vars';
        my $i = 0;
        my $sum = 0;
        while ($i < 10) {
            $i++;
            if ($i == 4) {
                last;
            }
            $sum += $i;
        }
        $sum * 10 + $i;"#,
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
        r#"no strict 'vars';
        my $hit = 0;
        OUTER: while (1) {
            my $j = 0;
            while ($j < 3) {
                $j++;
                if ($j == 2) {
                    last OUTER;
                }
                $hit++;
            }
        }
        $hit;"#,
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

/// `use strict` + all vars declared: VM path compiles and runs (previously bailed to tree).
#[test]
fn try_vm_execute_strict_vars_happy_path() {
    let p = parse(
        r#"use strict;
        use warnings;
        my $x = 5;
        my $y = 10;
        $x + $y;"#,
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
        r#"use strict;
        $undeclared = 5;
        $undeclared;"#,
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
        r#"use strict;
        sub sum {
            my $s = 0;
            for my $x (@_) {
                $s += $x;
            }
            return $s;
        }
        sum(1, 2, 3, 4, 5);"#,
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
        r#"use strict 'refs';
        my $name = "foo";
        my @a = @$name;
        $a[0];"#,
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
        r#"no strict 'vars';
        sub f {
            my $s = 0;
            $s += 5;
            $s *= 2;
            return $s;
        }
        f();"#,
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
        r#"no strict 'vars';
        sub f {
            my $s = "";
            $s .= "ab";
            $s .= "cd";
            return $s;
        }
        f();"#,
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
        r#"no strict 'vars';
        my $x = 0;
        goto END;
        $x = 99;
        END: $x;"#,
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
        r#"no strict 'vars';
        sub f {
            my $r = 10;
            goto SKIP;
            $r = 20;
            SKIP: return $r;
        }
        f();"#,
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
        r#"no strict 'vars';
        my $sum = 0;
        my $i = 0;
        LOOP: $sum = $sum + $i;
        $i = $i + 1;
        if ($i < 4) { goto LOOP; }
        $sum;"#,
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
        r#"no strict 'vars';
        my $x = 0;
        LOOP: $x = $x + 1;
        goto DONE;
        goto LOOP;
        DONE: $x;"#,
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
        r#"no strict 'vars';
        goto NO_SUCH;"#,
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
        r#"no strict 'vars';
        my $sum = 0;
        my $i = 0;
        while ($i < 5) {
            $sum += $i;
        } continue {
            $i++;
        }
        # sum = 0+1+2+3+4 = 10; i = 5
        $sum * 10 + $i;"#,
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
        r#"no strict 'vars';
        my $body = 0;
        my $cont = 0;
        foreach my $x (1..4) {
            $body += $x;
        } continue {
            $cont += $x;
        }
        # body = 1+2+3+4 = 10; cont = 10
        $body * 100 + $cont;"#,
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
        r#"no strict 'vars';
        my $cont_runs = 0;
        my $i = 0;
        while ($i < 3) {
            $i++;
            next;
        } continue {
            $cont_runs++;
        }
        # cont_runs should be 3 (continue runs per iteration even when next fires)
        $cont_runs;"#,
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
        r#"no strict 'vars';
        my $i = 0;
        my $cont_runs = 0;
        until ($i >= 3) {
            # body does nothing except guard
        } continue {
            $i++;
            $cont_runs++;
        }
        $i * 10 + $cont_runs;"#,
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
