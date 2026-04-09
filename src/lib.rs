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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_program() {
        let p = parse("").expect("empty input should parse");
        assert!(p.statements.is_empty());
    }

    #[test]
    fn parse_expression_statement() {
        let p = parse("2 + 2;").expect("parse");
        assert!(!p.statements.is_empty());
    }

    #[test]
    fn parse_semicolon_only_statements() {
        parse(";;;").expect("semicolons only");
    }

    #[test]
    fn parse_subroutine_declaration() {
        parse("sub foo { return 1; }").expect("sub");
    }

    #[test]
    fn parse_if_with_block() {
        parse("if (1) { 2 }").expect("if");
    }

    #[test]
    fn parse_fails_on_invalid_syntax() {
        assert!(parse("sub f {").is_err());
    }

    #[test]
    fn parse_qw_word_list() {
        parse("my @a = qw(x y z);").expect("qw list");
    }

    #[test]
    fn parse_c_style_for_loop() {
        parse("for (my $i = 0; $i < 3; $i = $i + 1) { 1; }").expect("c-style for");
    }

    #[test]
    fn parse_package_statement() {
        parse("package Foo::Bar; 1;").expect("package");
    }
}
