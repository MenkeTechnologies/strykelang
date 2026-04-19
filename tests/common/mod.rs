//! Shared helpers for integration tests. `cargo test` runs `tests/integration.rs` as its own crate
//! that imports this module and `tests/suite/*`.

use forge::error::ErrorKind;
use forge::interpreter::Interpreter;
use forge::value::PerlValue;

/// Parse and execute Perl code; panics on parse or runtime error.
pub fn eval(code: &str) -> PerlValue {
    let program = forge::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    interp.execute(&program).expect("execution failed")
}

pub fn eval_string(code: &str) -> String {
    eval(code).to_string()
}

pub fn eval_int(code: &str) -> i64 {
    eval(code).to_int()
}

pub fn eval_err_kind(code: &str) -> ErrorKind {
    let program = forge::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap_err().kind
}

pub fn parse_err_kind(code: &str) -> ErrorKind {
    forge::parse(code).unwrap_err().kind
}
