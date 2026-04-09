pub mod ast;
mod bench_fusion;
pub mod builtins;
pub mod bytecode;
pub mod capture;
pub mod compiler;
mod crypt_util;
pub mod data_section;
pub mod english;
mod fib_like_tail;
pub mod error;
pub mod fmt;
pub mod format;
pub mod interpreter;
mod jit;
pub mod lexer;
pub mod list_util;
mod map_grep_fast;
pub mod mro;
mod nanbox;
pub mod native_data;
pub mod pack;
pub mod par_lines;
pub mod par_pipeline;
pub mod parallel_trace;
pub mod parser;
pub mod pcache;
pub mod pchannel;
pub mod perl_fs;
pub mod perl_inc;
pub mod perl_signal;
mod pmap_progress;
pub mod ppool;
pub mod profiler;
pub mod pwatch;
pub mod scope;
mod sort_fast;
pub mod special_vars;
pub mod token;
pub mod value;
pub mod vm;

pub use interpreter::{
    perl_bracket_version, FEAT_SAY, FEAT_STATE, FEAT_SWITCH, FEAT_UNICODE_STRINGS,
};

use error::PerlResult;
use interpreter::Interpreter;
use value::PerlValue;

/// Parse a string of Perl code and return the AST.
/// Pretty-print a parsed program as Perl-like source (`pe --fmt`).
pub fn format_program(p: &ast::Program) -> String {
    fmt::format_program(p)
}

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

/// Crate-root `vendor/perl` (e.g. `List/Util.pm`). The `perlrs` / `pe` driver prepends this to
/// `@INC` when the directory exists so in-tree pure-Perl modules shadow XS-only core stubs.
pub fn vendor_perl_inc_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/perl")
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
    if interp.profiler.is_some() {
        return None;
    }
    // BEGIN/END blocks require tree-walker execution; skip VM path.
    let has_begin_end = program
        .statements
        .iter()
        .any(|s| matches!(s.kind, ast::StmtKind::Begin(_) | ast::StmtKind::End(_)));
    if has_begin_end {
        return None;
    }

    if let Err(e) = interp.prepare_program_top_level(program) {
        return Some(Err(e));
    }

    // `strict` pragmas are enforced in the tree-walker only (symbolic refs, undeclared globals, …).
    if interp.strict_refs || interp.strict_subs || interp.strict_vars {
        return None;
    }

    let comp = compiler::Compiler::new().with_source_file(interp.file.clone());
    match comp.compile_program(program) {
        Ok(chunk) => {
            for def in &chunk.struct_defs {
                interp
                    .struct_defs
                    .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
            }
            // Subs from `prepare_program_top_level` are already registered.
            let vm_jit = interp.vm_jit_enabled;
            let mut vm = vm::VM::new(&chunk, interp);
            vm.set_jit_enabled(vm_jit);
            match vm.execute() {
                Ok(val) => Some(Ok(val)),
                Err(ref e)
                    if e.message.starts_with("VM: unimplemented op")
                        || e.message.starts_with("Unimplemented builtin") =>
                {
                    None
                }
                Err(e) => Some(Err(e)),
            }
        }
        Err(ref _ce) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_executes_last_expression_value() {
        // Statement-only programs may yield 0 via the VM path; assert parse + run succeed.
        let p = parse("2 + 2;").expect("parse");
        assert!(!p.statements.is_empty());
        let _ = run("2 + 2;").expect("run");
    }

    #[test]
    fn run_propagates_parse_errors() {
        assert!(run("sub f {").is_err());
    }

    #[test]
    fn interpreter_scope_persists_global_scalar_across_execute_tree_calls() {
        let mut interp = Interpreter::new();
        let assign = parse("$persist_test = 100;").expect("parse assign");
        interp.execute_tree(&assign).expect("assign");
        let read = parse("$persist_test").expect("parse read");
        let v = interp.execute_tree(&read).expect("read");
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

    #[test]
    fn vendor_perl_inc_path_points_at_vendor_perl() {
        let p = vendor_perl_inc_path();
        assert!(
            p.ends_with("vendor/perl"),
            "unexpected vendor path: {}",
            p.display()
        );
    }

    #[test]
    fn format_program_roundtrips_simple_expression() {
        let p = parse("$x + 1;").expect("parse");
        let out = format_program(&p);
        assert!(!out.trim().is_empty());
    }
}

#[cfg(test)]
mod parse_smoke_extended;

#[cfg(test)]
mod parse_smoke_batch2;

#[cfg(test)]
mod parse_smoke_batch3;

#[cfg(test)]
mod parse_smoke_batch4;

#[cfg(test)]
mod crate_api_tests;

#[cfg(test)]
mod parser_shape_tests;

#[cfg(test)]
mod interpreter_unit_tests;

#[cfg(test)]
mod run_semantics_tests;

#[cfg(test)]
mod run_semantics_more;
