//! Public crate API: `forge::run`, `parse`/`format_program`/`lint_program`, `try_vm_execute`,
//! `parse_and_run_string`, `parse_and_run_string_in_file`, and `parse_with_file` diagnostics.

use forge::error::ErrorKind;
use forge::interpreter::Interpreter;
use forge::{
    format_program, lint_program, parse, parse_and_run_string, parse_and_run_string_in_file,
    parse_with_file, run, try_vm_execute, vendor_perl_inc_path,
};

#[test]
fn run_returns_computed_integer() {
    assert_eq!(run("17 - 4").expect("run").to_int(), 13);
}

#[test]
fn run_value_is_last_statement_result() {
    assert_eq!(run("1; 2; 3").expect("run").to_int(), 3);
}

#[test]
fn run_returns_err_on_invalid_syntax() {
    assert!(run("}").is_err());
}

#[test]
fn parse_and_run_string_returns_err_on_invalid_syntax() {
    let mut interp = Interpreter::new();
    assert!(parse_and_run_string("}", &mut interp).is_err());
}

#[test]
fn run_last_expression_string_value() {
    assert_eq!(run(r#"my $s = "ab"; $s"#).expect("run").to_string(), "ab",);
}

#[test]
fn run_returns_err_on_division_by_zero() {
    let e = run("1/0").expect_err("runtime error");
    assert_eq!(e.kind, ErrorKind::Runtime);
}

#[test]
fn run_returns_err_on_die() {
    let e = run(r#"die "stop""#).expect_err("die");
    assert_eq!(e.kind, ErrorKind::Die);
}

#[test]
fn parse_and_run_string_returns_err_on_runtime_failure() {
    let mut interp = Interpreter::new();
    let r = parse_and_run_string("1/0", &mut interp);
    assert!(r.is_err());
}

#[test]
fn parse_and_run_string_accumulates_state_across_calls() {
    let mut interp = Interpreter::new();
    parse_and_run_string("my $x = 10;", &mut interp).expect("first");
    let v = parse_and_run_string("$x + 5", &mut interp).expect("second");
    assert_eq!(v.to_int(), 15);
}

#[test]
fn parse_and_run_string_returns_last_statement_value() {
    let mut interp = Interpreter::new();
    let v = parse_and_run_string("1; 2; 7", &mut interp).expect("run");
    assert_eq!(v.to_int(), 7);
}

#[test]
fn parse_and_run_string_preserves_subroutine_definitions() {
    let mut interp = Interpreter::new();
    parse_and_run_string("sub api_t { return 40 + 2; }", &mut interp).expect("define");
    let v = parse_and_run_string("api_t()", &mut interp).expect("call");
    assert_eq!(v.to_int(), 42);
}

#[test]
fn parse_and_run_string_in_file_magic_file_matches_argument() {
    let mut interp = Interpreter::new();
    interp.set_file("caller.pm");
    let v =
        parse_and_run_string_in_file("__FILE__", &mut interp, "/tmp/module/Foo.pm").expect("run");
    assert_eq!(v.to_string(), "/tmp/module/Foo.pm");
    let after = parse_and_run_string("__FILE__", &mut interp).expect("file after");
    assert_eq!(after.to_string(), "caller.pm");
}

#[test]
fn parse_and_run_string_in_file_restores_interp_file_after_success() {
    let mut interp = Interpreter::new();
    parse_and_run_string_in_file("1 + 1", &mut interp, "other.pl").expect("run");
    let v = parse_and_run_string("__FILE__", &mut interp).expect("file");
    assert_eq!(v.to_string(), "-e");
}

#[test]
fn parse_and_run_string_in_file_restores_interp_file_after_runtime_error() {
    let mut interp = Interpreter::new();
    interp.set_file("caller.pl");
    let r = parse_and_run_string_in_file(r#"die "stop""#, &mut interp, "evalunit.pl");
    assert!(r.is_err());
    let after = parse_and_run_string("__FILE__", &mut interp).expect("file after die");
    assert_eq!(after.to_string(), "caller.pl");
}

#[test]
fn parse_and_run_string_in_file_parse_error_leaves_interp_file_unchanged() {
    let mut interp = Interpreter::new();
    interp.set_file("caller.pl");
    assert!(parse_and_run_string_in_file("sub x {", &mut interp, "broken.pl").is_err());
    let after = parse_and_run_string("__FILE__", &mut interp).expect("file after parse err");
    assert_eq!(after.to_string(), "caller.pl");
}

#[test]
fn parse_with_file_threads_path_into_syntax_error() {
    let e = parse_with_file("}", "lib/MyMod.pm").expect_err("syntax");
    assert_eq!(e.kind, ErrorKind::Syntax);
    assert_eq!(e.file, "lib/MyMod.pm");
}

#[test]
fn format_program_output_reparses() {
    let p = parse("my $fmt_y = 2; $fmt_y + 2").expect("parse");
    let out = format_program(&p);
    let _ = parse(&out).expect("formatted source should parse");
    assert!(!out.trim().is_empty());
}

#[test]
fn lint_program_ok_for_simple_vm_compilable_program() {
    let p = parse("my $lint_x = 1; $lint_x + 1").expect("parse");
    let mut interp = Interpreter::new();
    lint_program(&p, &mut interp).expect("lint");
}

#[test]
fn try_vm_execute_matches_run_for_simple_expression() {
    let code = "11 * 3;";
    let program = parse(code).expect("parse");
    let mut interp = Interpreter::new();
    let vm_val = try_vm_execute(&program, &mut interp)
        .expect("expected VM to compile simple expression")
        .expect("vm execution");
    assert_eq!(vm_val.to_int(), run(code).expect("run").to_int());
}

#[test]
fn vendor_perl_inc_path_suffix_is_vendor_perl() {
    let p = vendor_perl_inc_path();
    assert!(
        p.ends_with("vendor/perl"),
        "unexpected vendor path: {}",
        p.display()
    );
}

#[test]
fn try_vm_execute_sub_def_visible_to_parse_and_run_string() {
    let mut interp = Interpreter::new();
    let def = parse("sub lib_api_vm_sub { 41 }").expect("parse sub");
    try_vm_execute(&def, &mut interp)
        .expect("vm compiles sub")
        .expect("vm run");
    let v = parse_and_run_string("lib_api_vm_sub() + 1", &mut interp).expect("follow-up");
    assert_eq!(v.to_int(), 42);
}
