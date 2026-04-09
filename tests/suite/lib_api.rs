//! Public crate API: `perlrs::run` and `parse_and_run_string` with shared `Interpreter`.

use perlrs::error::ErrorKind;
use perlrs::interpreter::Interpreter;
use perlrs::{parse_and_run_string, run};

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
    assert_eq!(
        run(r#"my $s = "ab"; $s"#).expect("run").to_string(),
        "ab",
    );
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
