//! Typeglob assignment (`*foo = \\&bar`, `*foo = *bar`) for subroutine aliasing and stash copy.

use crate::common::*;

#[test]
fn typeglob_assign_coderef_installs_sub_alias() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            sub orig { 41 }
            *alias = \&orig;
            alias() + 1"#,
        ),
        42
    );
}

#[test]
fn typeglob_assign_glob_copies_subroutine_slot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            sub first_fn { 7 }
            *two = *first_fn;
            two() * 2"#,
        ),
        14
    );
}

#[test]
fn typeglob_parse_simple_glob_assign_statement() {
    let p = stryke::parse("*a = *b").expect("parse");
    assert!(!p.statements.is_empty());
}

#[test]
fn typeglob_parse_package_qualified_name() {
    let p = stryke::parse("*Foo::x").expect("parse");
    assert!(!p.statements.is_empty());
}

#[test]
fn typeglob_parse_qualified_glob_assign() {
    let p = stryke::parse("*Foo::x = *Foo::y").expect("parse");
    assert!(!p.statements.is_empty());
}

#[test]
fn typeglob_assign_anonymous_sub_empty_prototype_parses() {
    // Carp.pm-style: *NAME = sub () { 1 };
    let p = stryke::parse("no strict; *x = fn () { 1 }").expect("parse");
    assert!(!p.statements.is_empty());
}

#[test]
fn vm_compiles_dynamic_typeglob_expr() {
    // Exporter.pm-style: *{"$pkg::$sym"} = ...
    let code = r#"no strict 'vars';
        *{"STDOUT"};"#;
    let program = stryke::parse(code).expect("parse");
    let mut interp = stryke::interpreter::Interpreter::new();
    assert!(
        stryke::try_vm_execute(&program, &mut interp).is_some(),
        "expected bytecode VM for dynamic typeglob (Op::LoadDynamicTypeglob)"
    );
}
