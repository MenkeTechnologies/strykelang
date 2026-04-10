pub mod ast;
pub mod builtins;
pub mod bytecode;
pub mod capture;
pub mod compiler;
mod crypt_util;
pub mod data_section;
pub mod english;
pub mod error;
mod fib_like_tail;
pub mod fmt;
pub mod format;
pub mod interpreter;
mod jit;
pub mod lexer;
pub mod list_util;
mod map_grep_fast;
pub mod mro;
mod nanbox;
mod native_codec;
pub mod native_data;
pub mod pack;
pub mod par_lines;
pub mod par_pipeline;
pub mod par_walk;
pub mod parallel_trace;
pub mod parser;
pub mod pcache;
pub mod pchannel;
pub mod perl_fs;
pub mod perl_inc;
mod perl_regex;
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

use error::{PerlError, PerlResult};
use interpreter::Interpreter;
use value::PerlValue;

/// Parse a string of Perl code and return the AST.
/// Pretty-print a parsed program as Perl-like source (`pe --fmt`).
pub fn format_program(p: &ast::Program) -> String {
    fmt::format_program(p)
}

pub fn parse(code: &str) -> PerlResult<ast::Program> {
    parse_with_file(code, "-e")
}

/// Parse with a **source path** for lexer/parser diagnostics (`… at FILE line N`), e.g. a script
/// path or a required `.pm` absolute path. Use [`parse`] for snippets where `-e` is appropriate.
pub fn parse_with_file(code: &str, file: &str) -> PerlResult<ast::Program> {
    let mut lexer = lexer::Lexer::new_with_file(code, file);
    let tokens = lexer.tokenize()?;
    let mut parser = parser::Parser::new_with_file(tokens, file);
    parser.parse_program()
}

/// Parse and execute a string of Perl code within an existing interpreter.
/// Tries bytecode VM first, falls back to tree-walker on unsupported features.
/// Uses [`Interpreter::file`] for both parse diagnostics and `__FILE__` during this execution.
pub fn parse_and_run_string(code: &str, interp: &mut Interpreter) -> PerlResult<PerlValue> {
    let file = interp.file.clone();
    parse_and_run_string_in_file(code, interp, &file)
}

/// Like [`parse_and_run_string`], but parse errors and `__FILE__` for this run use `file` (e.g. a
/// required module path). Restores [`Interpreter::file`] after execution.
pub fn parse_and_run_string_in_file(
    code: &str,
    interp: &mut Interpreter,
    file: &str,
) -> PerlResult<PerlValue> {
    let program = parse_with_file(code, file)?;
    let saved = interp.file.clone();
    interp.file = file.to_string();
    let r = interp.execute(&program);
    interp.file = saved;
    r
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
    if let Err(e) = interp.prepare_program_top_level(program) {
        return Some(Err(e));
    }

    // `use strict 'vars'` is enforced at compile time by the compiler (see
    // `Compiler::check_strict_scalar_access` and siblings). `strict refs` / `strict subs` are
    // enforced by the tree helpers that the VM already delegates into (symbolic deref,
    // `call_named_sub`, etc.), so they work transitively.
    let comp = compiler::Compiler::new()
        .with_source_file(interp.file.clone())
        .with_strict_vars(interp.strict_vars);
    match comp.compile_program(program) {
        Ok(chunk) => {
            if interp.disasm_bytecode {
                eprintln!("{}", chunk.disassemble());
            }
            // BEGIN/END are emitted in the chunk; avoid running them again from
            // [`Interpreter::begin_blocks`] / [`Interpreter::end_blocks`] if anything used the tree path.
            interp.clear_begin_end_blocks_after_vm_compile();
            for def in &chunk.struct_defs {
                interp
                    .struct_defs
                    .insert(def.name.clone(), std::sync::Arc::new(def.clone()));
            }
            // Subs from `prepare_program_top_level` are already registered.
            // Profiling attributes wall time to opcodes and call/return pairs; JIT would skip both.
            let vm_jit = interp.vm_jit_enabled && interp.profiler.is_none();
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
        // `CompileError::Frozen` is a hard compile-time error (strict pragma violations, frozen
        // lvalue writes, unknown goto labels). Promote it to a user-visible runtime error so
        // the VM path matches `perl` — without this promotion the fallback would run the tree
        // interpreter, which sometimes silently accepts the same construct (e.g. strict_vars
        // isn't enforced on scalar assignment in the tree path).
        Err(compiler::CompileError::Frozen { line, detail }) => {
            Some(Err(PerlError::runtime(detail, line)))
        }
        // `Unsupported` just means "this VM compiler doesn't handle this construct yet" — fall
        // back to the tree interpreter.
        Err(compiler::CompileError::Unsupported(_)) => None,
    }
}

/// Parse + register top-level subs / `use` (same as the VM path), then compile to bytecode without running.
/// When `strict` pragmas are enabled, bytecode compilation is skipped (same limitation as [`try_vm_execute`]).
pub fn lint_program(program: &ast::Program, interp: &mut Interpreter) -> PerlResult<()> {
    if let Err(e) = interp.prepare_program_top_level(program) {
        return Err(e);
    }
    if interp.strict_refs || interp.strict_subs || interp.strict_vars {
        eprintln!("perlrs: warning: bytecode compile check skipped (strict pragma is enabled)");
        return Ok(());
    }
    let comp = compiler::Compiler::new().with_source_file(interp.file.clone());
    match comp.compile_program(program) {
        Ok(_) => Ok(()),
        Err(e) => Err(compile_error_to_perl(e)),
    }
}

fn compile_error_to_perl(e: compiler::CompileError) -> PerlError {
    match e {
        compiler::CompileError::Unsupported(msg) => {
            PerlError::runtime(format!("compile: {}", msg), 0)
        }
        compiler::CompileError::Frozen { line, detail } => PerlError::runtime(detail, line),
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
