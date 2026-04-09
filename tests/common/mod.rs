//! Shared helpers for integration tests. `cargo test` runs `tests/integration.rs` as its own crate
//! that imports this module and `tests/suite/*`.

use perlrs::error::ErrorKind;
use perlrs::interpreter::Interpreter;
use perlrs::value::PerlValue;

/// Parse and execute Perl code; panics on parse or runtime error.
pub fn eval(code: &str) -> PerlValue {
    let program = perlrs::parse(code).expect("parse failed");
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
    let program = perlrs::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    interp.execute(&program).unwrap_err().kind
}

pub fn parse_err_kind(code: &str) -> ErrorKind {
    perlrs::parse(code).unwrap_err().kind
}
