//! `&sub` / `\&sub` / `\&{ EXPR }` lowered to VM (`Op::Call` / `Op::LoadNamedSubRef` / `Op::LoadDynamicSubRef`).

use crate::common::*;
use stryke::interpreter::Interpreter;

#[test]
fn ampersand_sub_invokes_named_sub() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            sub tally { 40 }
            tally() + &tally"#,
        ),
        80
    );
}

#[test]
fn backslash_ampersand_yields_coderef_and_call() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            sub n { 11 }
            my $c = \&n;
            $c->() * 2"#,
        ),
        22
    );
}

#[test]
fn vm_program_compiles_subroutine_code_ref() {
    let code = r#"no strict 'vars';
        sub f { 1 }
        \&f"#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = Interpreter::new();
    assert!(
        stryke::try_vm_execute(&program, &mut interp).is_some(),
        "expected bytecode VM for \\\\&f expression"
    );
}

#[test]
fn vm_program_compiles_dynamic_subroutine_coderef() {
    let code = r#"no strict 'vars';
        sub g { 7 }
        \&{"g"}"#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = Interpreter::new();
    assert!(
        stryke::try_vm_execute(&program, &mut interp).is_some(),
        "expected bytecode VM for Op::LoadDynamicSubRef (dynamic coderef)"
    );
}
