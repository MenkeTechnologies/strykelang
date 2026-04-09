pub mod ast;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod scope;
pub mod token;
pub mod value;

use error::PerlResult;
use interpreter::Interpreter;
use value::PerlValue;

/// Parse a string of Perl code and return the AST.
pub fn parse(code: &str) -> PerlResult<ast::Program> {
    let mut lexer = lexer::Lexer::new(code);
    let tokens = lexer.tokenize()?;
    let mut parser = parser::Parser::new(tokens);
    parser.parse_program()
}

/// Parse and execute a string of Perl code within an existing interpreter.
pub fn parse_and_run_string(code: &str, interp: &mut Interpreter) -> PerlResult<PerlValue> {
    let program = parse(code)?;
    interp.execute(&program)
}

/// Parse and execute a string of Perl code with a fresh interpreter.
pub fn run(code: &str) -> PerlResult<PerlValue> {
    let program = parse(code)?;
    let mut interp = Interpreter::new();
    interp.execute(&program)
}
