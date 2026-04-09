//! Unit tests for `Interpreter`: defaults, `set_file`, and `execute_tree` behavior.

use crate::ast::StmtKind;
use crate::interpreter::Interpreter;
use crate::parse;
use crate::value::PerlValue;

#[test]
fn our_isa_stores_c_isa_for_parents_of_class() {
    let mut i = Interpreter::new();
    let prog = parse("package C; our @ISA = qw(P); 1;").unwrap();
    i.execute_tree(&prog).unwrap();
    assert_eq!(i.parents_of_class("C"), vec!["P".to_string()]);
}

#[test]
fn super_fixture_succeeds_on_tree_execute_path() {
    let mut i = Interpreter::new();
    let prog = parse(
        r#"
        package P;
        sub meth { 10 }
        package C;
        our @ISA = qw(P);
        sub meth { my $s = shift; $s->SUPER::meth + 5 }
        package main;
        my $o = bless {}, "C";
        $o->meth();
    "#,
    )
    .unwrap();
    let v = i.execute_tree(&prog).expect("execute_tree");
    assert_eq!(v.to_int(), 15);
}

#[test]
fn new_default_file_is_dash_e() {
    assert_eq!(Interpreter::new().file, "-e");
}

#[test]
fn new_default_program_name() {
    assert_eq!(Interpreter::new().program_name, "perlrs");
}

#[test]
fn new_default_irs_newline() {
    assert_eq!(Interpreter::new().irs, "\n");
}

#[test]
fn new_line_number_starts_zero() {
    assert_eq!(Interpreter::new().line_number, 0);
}

#[test]
fn new_env_populated_from_process() {
    let mut i = Interpreter::new();
    i.materialize_env_if_needed();
    assert!(
        i.env.contains_key("PATH") || i.env.contains_key("HOME") || !i.env.is_empty(),
        "expected some process env in interpreter env"
    );
}

#[test]
fn set_file_updates_file_field() {
    let mut i = Interpreter::new();
    i.set_file("t/foo.pl");
    assert_eq!(i.file, "t/foo.pl");
}

#[test]
fn execute_tree_computed_expression() {
    let p = parse("7 * 6;").expect("parse");
    let mut i = Interpreter::new();
    let v = i.execute_tree(&p).expect("execute_tree");
    assert_eq!(v.to_int(), 42);
}

#[test]
fn execute_tree_my_scalar_sequence() {
    let p = parse("my $a = 10; my $b = 32; $a + $b;").expect("parse");
    let mut i = Interpreter::new();
    let v = i.execute_tree(&p).expect("execute_tree");
    assert_eq!(v.to_int(), 42);
}

#[test]
fn execute_tree_registers_sub_for_later_call() {
    let p = parse("sub times6 { return $_[0] * 6; } times6(7);").expect("parse");
    let mut i = Interpreter::new();
    let v = i.execute_tree(&p).expect("execute_tree");
    assert_eq!(v.to_int(), 42);
}

#[test]
fn execute_preserves_scope_scalar_across_two_parses() {
    let p1 = parse("my $interp_unit_x = 41;").expect("parse");
    let p2 = parse("$interp_unit_x + 1;").expect("parse");
    let mut i = Interpreter::new();
    i.execute_tree(&p1).expect("first");
    let v = i.execute_tree(&p2).expect("second");
    assert_eq!(v.to_int(), 42);
}

#[test]
fn subs_map_holds_declared_sub() {
    let p = parse("sub interp_named { 1 }").expect("parse");
    let mut i = Interpreter::new();
    i.execute_tree(&p).expect("execute_tree");
    assert!(i.subs.contains_key("interp_named"));
}

#[test]
fn format_decl_registers_template_and_render_matches_picture() {
    let mut i = Interpreter::new();
    let prog = parse(
        r#"
format STDOUT =
@<<<< @>>>>
1, 2
.
1;
"#,
    )
    .expect("parse");
    let format_lines = prog
        .statements
        .iter()
        .find_map(|s| match &s.kind {
            StmtKind::FormatDecl { lines, .. } => Some(lines.clone()),
            _ => None,
        })
        .expect("format decl");
    assert_eq!(
        format_lines,
        vec!["@<<<< @>>>>".to_string(), "1, 2".to_string()],
        "format body lines should be picture then value"
    );
    i.prepare_program_top_level(&prog).expect("prepare");
    let tmpl = i
        .format_templates
        .get("main::STDOUT")
        .cloned()
        .expect("format registered under package::NAME");
    let out = i
        .render_format_template(tmpl.as_ref(), 1)
        .expect("render");
    // Picture `@<<<< @>>>>` is two 4-wide fields with a literal space between.
    assert_eq!(out, "1       2\n");
}

#[test]
fn list_separator_dollar_quote_roundtrips() {
    let mut i = Interpreter::new();
    assert_eq!(i.list_separator, " ");
    i.set_special_var("\"", &PerlValue::string(",".into()))
        .expect("set $\"");
    assert_eq!(i.get_special_var("\"").to_string(), ",");
    assert_eq!(i.list_separator, ",");
}

#[test]
fn caret_unicode_preseeded_undef() {
    let i = Interpreter::new();
    assert!(i.get_special_var("^UNICODE").is_undef());
}

#[test]
fn star_multiline_prepends_dotall_in_compile_regex() {
    let mut i = Interpreter::new();
    i.set_special_var("*", &PerlValue::integer(1)).expect("set $*");
    let re = i.compile_regex("a.b", "", 1).expect("compile");
    assert!(re.is_match("a\nb"));
    i.set_special_var("*", &PerlValue::integer(0)).expect("clear $*");
    let re2 = i.compile_regex("a.b", "", 1).expect("compile");
    assert!(!re2.is_match("a\nb"));
}

#[test]
fn stash_array_caret_prefixed_stays_global() {
    let mut i = Interpreter::new();
    let _ = i
        .scope
        .set_scalar("__PACKAGE__", PerlValue::string("Foo".into()));
    assert_eq!(i.stash_array_name_for_package("^CAPTURE"), "^CAPTURE");
}

#[test]
fn at_is_dualvar_after_eval_failure() {
    let mut i = Interpreter::new();
    let prog = parse(r#"eval("1/0"); 0+$@"#).expect("parse");
    let v = i.execute_tree(&prog).expect("execute_tree");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn at_dualvar_roundtrip_assignment() {
    let mut i = Interpreter::new();
    let dv = PerlValue::errno_dual(7, "x".into());
    i.set_special_var("@", &dv).expect("set $@");
    assert_eq!(i.eval_error_code, 7);
    assert_eq!(i.eval_error, "x");
    let g = i.get_special_var("@");
    assert_eq!(g.to_int(), 7);
    assert_eq!(g.to_string(), "x");
}

#[test]
fn at_clear_eval_error_zeroes_dualvar_read() {
    let mut i = Interpreter::new();
    i.set_eval_error("err".into());
    i.clear_eval_error();
    let g = i.get_special_var("@");
    assert_eq!(g.to_int(), 0);
    assert_eq!(g.to_string(), "");
}

#[test]
fn set_eval_error_empty_string_clears_code() {
    let mut i = Interpreter::new();
    i.set_eval_error("x".into());
    assert_eq!(i.eval_error_code, 1);
    i.set_eval_error(String::new());
    assert_eq!(i.eval_error_code, 0);
    assert!(i.eval_error.is_empty());
}

#[test]
fn at_set_special_plain_string_uses_code_one_when_nonnumeric() {
    let mut i = Interpreter::new();
    i.set_special_var("@", &PerlValue::string("boom".into()))
        .expect("set $@");
    assert_eq!(i.eval_error_code, 1);
    assert_eq!(i.eval_error, "boom");
}

#[test]
fn at_set_special_integer_keeps_numeric_code_and_display_string() {
    let mut i = Interpreter::new();
    i.set_special_var("@", &PerlValue::integer(99)).expect("set $@");
    assert_eq!(i.eval_error_code, 99);
    assert_eq!(i.eval_error, "99");
}

#[test]
fn at_set_special_string_zero_still_gets_code_one() {
    // Non-empty message with numeric parse 0 → `set_special_var` bumps code to 1.
    let mut i = Interpreter::new();
    i.set_special_var("@", &PerlValue::string("0".into()))
        .expect("set $@");
    assert_eq!(i.eval_error_code, 1);
    assert_eq!(i.eval_error, "0");
}

#[test]
fn capture_array_after_bind_match() {
    let mut i = Interpreter::new();
    let prog = parse(r#""foo=bar" =~ /=(.*)/; 1"#).expect("parse");
    i.execute_tree(&prog).expect("execute_tree");
    let cap = i.scope.get_array("^CAPTURE");
    assert_eq!(cap.len(), 1);
    assert_eq!(cap[0].to_string(), "bar");
}
