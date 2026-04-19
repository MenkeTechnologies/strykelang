//! High-impact Perl globals: `$]`, `$;`, `$^*`, `$ARGV` + `<>`, `@-` / `@+`, `$^S`, `%INC` / `%SIG`.

use crate::common::*;
use forge::interpreter::Interpreter;
use forge::parse;
use forge::perl_bracket_version;
use forge::value::PerlValue;

#[test]
fn bracket_version_matches_helper() {
    let v = eval("$]");
    assert!((v.to_number() - perl_bracket_version()).abs() < 1e-9);
}

#[test]
fn subscript_separator_default() {
    assert_eq!(eval_string("$;"), "\x1c");
}

#[test]
fn caret_specials_lex_and_roundtrip() {
    assert_eq!(eval_string("$^I"), "");
    assert_eq!(eval_int("$^D"), 0);
    assert_eq!(eval_int("$^P"), 0);
    assert_eq!(eval_int("$^W"), 0);
}

#[test]
fn hat_s_false_at_top_level_true_inside_eval_block() {
    assert_eq!(eval_int("$^S"), 0);
    assert_eq!(eval_int("eval { $^S }"), 1);
}

#[test]
fn match_arrays_minus_plus_offsets() {
    assert_eq!(
        eval_int(r#"my $s = "ab"; $s =~ /(a)(b)/; $-[0] + $-[1] * 10 + $-[2] * 100"#),
        100
    );
    assert_eq!(
        eval_int(r#"my $s = "ab"; $s =~ /(a)(b)/; $+[0] + $+[1] * 10 + $+[2] * 100"#),
        2 + 10 + 2 * 100
    );
}

#[test]
fn diamond_sets_argv_scalar() {
    let base = std::env::temp_dir().join(format!("forge_diamond_{}.txt", std::process::id()));
    std::fs::write(&base, "hello\n").expect("write temp");
    let path = base.to_string_lossy().into_owned();

    let program = parse("my $line = <>; length($line) + length($ARGV)").expect("parse");
    let mut interp = Interpreter::new();
    interp
        .scope
        .declare_array("ARGV", vec![PerlValue::string(path.clone())]);
    let v = interp.execute(&program).expect("run");
    assert_eq!(v.to_int(), 6 + path.len() as i64);
    assert_eq!(interp.argv_current_file, path);
    let _ = std::fs::remove_file(&base);
}

#[test]
fn dollar_pipe_autoflush_toggle() {
    assert_eq!(eval_int("$|"), 0);
    assert_eq!(eval_int("$| = 1; $|"), 1);
    assert_eq!(eval_int("$| = 0; $|"), 0);
}

#[test]
fn require_populates_inc_hash() {
    let d = format!("{}/tests/fixtures/inc", env!("CARGO_MANIFEST_DIR"));
    let program = parse(&format!(
        r#"
        @INC = ("{d}");
        require Trivial;
        scalar(keys %INC);
    "#
    ))
    .expect("parse");
    let mut interp = Interpreter::new();
    let v = interp.execute(&program).expect("run");
    assert!(v.to_int() >= 1);
    assert!(interp.scope.exists_hash_element("INC", "Trivial.pm"));
}
