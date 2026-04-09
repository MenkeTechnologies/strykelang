//! `@INC`, `%INC`, `require`, and `use` loading (pure perlrs `.pm` files).

use perlrs::interpreter::Interpreter;
use perlrs::value::PerlValue;
use perlrs::{parse, parse_and_run_string};

fn fixture_inc() -> String {
    format!("{}/tests/fixtures/inc", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn require_loads_pm_and_sets_inc() {
    let mut interp = Interpreter::new();
    let d = fixture_inc();
    interp.scope.declare_array(
        "INC",
        vec![PerlValue::String(d), PerlValue::String(".".to_string())],
    );
    let p = parse("require Trivial; Trivial::trivial_answer();").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 42);
    assert!(interp.scope.exists_hash_element("INC", "Trivial.pm"));
}

#[test]
fn use_loads_module_and_second_require_is_noop() {
    let mut interp = Interpreter::new();
    let d = fixture_inc();
    interp.scope.declare_array(
        "INC",
        vec![PerlValue::String(d), PerlValue::String(".".to_string())],
    );
    // `use` runs in prepare (before main); module subs are visible in main body.
    let p = parse(
        "use Trivial;\n\
         trivial_answer() + 1;",
    )
    .expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 43);

    let mut interp2 = Interpreter::new();
    interp2.scope.declare_array(
        "INC",
        vec![
            PerlValue::String(fixture_inc()),
            PerlValue::String(".".to_string()),
        ],
    );
    let p2 = parse("require Trivial; require Trivial; 7;").expect("parse");
    let v2 = interp2.execute(&p2).expect("run");
    assert_eq!(v2.to_int(), 7);
}

#[test]
fn use_trivial_qw_imports_symbol() {
    let mut interp = Interpreter::new();
    interp.scope.declare_array(
        "INC",
        vec![
            PerlValue::String(fixture_inc()),
            PerlValue::String(".".to_string()),
        ],
    );
    let p = parse("use Trivial qw(trivial_answer); trivial_answer() + 1;").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 43);
}

#[test]
fn use_trivial_empty_list_does_not_import() {
    let mut interp = Interpreter::new();
    interp.scope.declare_array(
        "INC",
        vec![
            PerlValue::String(fixture_inc()),
            PerlValue::String(".".to_string()),
        ],
    );
    let p = parse("use Trivial qw(); trivial_answer();").expect("parse");
    assert!(interp.execute(&p).is_err());
}

#[test]
fn parse_and_run_string_nested_require_shares_inc() {
    let mut interp = Interpreter::new();
    interp.scope.declare_array(
        "INC",
        vec![
            PerlValue::String(fixture_inc()),
            PerlValue::String(".".to_string()),
        ],
    );
    parse_and_run_string("require Trivial;", &mut interp).expect("req");
    let v = parse_and_run_string("Trivial::trivial_answer();", &mut interp).expect("call");
    assert_eq!(v.to_int(), 42);
}
