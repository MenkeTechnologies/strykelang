pub mod ast;
pub mod bytecode;
pub mod compiler;
pub mod error;
pub mod interpreter;
pub mod lexer;
pub mod parser;
pub mod scope;
pub mod token;
pub mod value;
pub mod vm;

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
/// Tries bytecode VM first, falls back to tree-walker on unsupported features.
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

/// Try to compile and run via bytecode VM. Returns None if compilation fails.
pub fn try_vm_execute(
    program: &ast::Program,
    interp: &mut Interpreter,
) -> Option<PerlResult<PerlValue>> {
    let comp = compiler::Compiler::new();
    match comp.compile_program(program) {
        Ok(chunk) => {
            let mut vm = vm::VM::new(&chunk, interp);
            Some(vm.execute())
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_executes_last_expression_value() {
        assert_eq!(run("2 + 2").expect("run").to_int(), 4);
    }

    #[test]
    fn run_propagates_parse_errors() {
        assert!(run("sub f {").is_err());
    }

    #[test]
    fn parse_and_run_string_shares_one_interpreter_state() {
        let mut interp = Interpreter::new();
        parse_and_run_string("sub bar { return 100; }", &mut interp).expect("define sub");
        let v = parse_and_run_string("bar()", &mut interp).expect("call sub");
        assert_eq!(v.to_int(), 100);
    }

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

    #[test]
    fn parse_unless_block() {
        parse("unless (0) { 1; }").expect("unless");
    }

    #[test]
    fn parse_if_elsif_else() {
        parse("if (0) { 1; } elsif (1) { 2; } else { 3; }").expect("if elsif");
    }

    #[test]
    fn parse_q_constructor() {
        parse(r#"my $s = q{braces};"#).expect("q{}");
        parse(r#"my $t = qq(double);"#).expect("qq()");
    }

    #[test]
    fn parse_regex_literals() {
        parse("m/foo/;").expect("m//");
        parse("s/foo/bar/g;").expect("s///");
    }

    #[test]
    fn parse_begin_and_end_blocks() {
        parse("BEGIN { 1; }").expect("BEGIN");
        parse("END { 1; }").expect("END");
    }

    #[test]
    fn parse_transliterate_y() {
        parse("$_ = 'a'; y/a/A/;").expect("y//");
    }

    #[test]
    fn parse_foreach_with_my_iterator() {
        parse("foreach my $x (1, 2) { $x; }").expect("foreach my");
    }

    #[test]
    fn parse_our_declaration() {
        parse("our $g = 1;").expect("our");
    }

    #[test]
    fn parse_local_declaration() {
        parse("local $x = 1;").expect("local");
    }

    #[test]
    fn parse_use_no_statements() {
        parse("use strict;").expect("use");
        parse("no warnings;").expect("no");
    }

    #[test]
    fn parse_sub_with_prototype() {
        parse("sub sum ($$) { return $_[0] + $_[1]; }").expect("sub prototype");
    }

    #[test]
    fn parse_list_expression_in_parentheses() {
        parse("my @a = (1, 2, 3);").expect("list");
    }

    #[test]
    fn parse_require_expression() {
        parse("require strict;").expect("require");
    }

    #[test]
    fn parse_do_string_eval_form() {
        parse(r#"do "foo.pl";"#).expect("do string");
    }

    #[test]
    fn parse_package_qualified_name() {
        parse("package Foo::Bar::Baz;").expect("package ::");
    }

    #[test]
    fn parse_my_multiple_declarations() {
        parse("my ($a, $b, $c);").expect("my list");
    }

    #[test]
    fn parse_eval_block_statement() {
        parse("eval { 1; };").expect("eval block");
    }

    #[test]
    fn parse_say_statement() {
        parse("say 42;").expect("say");
    }

    #[test]
    fn parse_chop_scalar() {
        parse("chop $s;").expect("chop");
    }
}
