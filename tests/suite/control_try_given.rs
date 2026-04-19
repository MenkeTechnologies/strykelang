//! `try`/`catch`, `given`/`when`/`default`, and `eval_timeout` (tree interpreter).

use stryke::ast::StmtKind;
use stryke::parse;
use stryke::run;
#[test]
fn parse_try_catch_shape() {
    let p = parse("try { 1; } catch ($err) { 2; }").expect("parse");
    assert!(matches!(p.statements[0].kind, StmtKind::TryCatch { .. }));
}

#[test]
fn parse_given_when_default_shape() {
    let p = parse(
        r#"given (1) {
        when (1) { 10; }
        default { 0; }
    }"#,
    )
    .expect("parse");
    assert!(matches!(p.statements[0].kind, StmtKind::Given { .. }));
}

#[test]
fn parse_eval_timeout_shape() {
    let p = parse("eval_timeout 5 { 1; }").expect("parse");
    assert!(matches!(p.statements[0].kind, StmtKind::EvalTimeout { .. }));
}

#[test]
fn try_catch_runs_catch_on_die() {
    let v = run(r#"
        try {
            die "boom";
        } catch ($err) {
            42;
        }
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 42);
}

#[test]
fn parse_try_catch_finally_shape() {
    let p = parse("try { 1; } catch ($err) { 2; } finally { 3; }").expect("parse");
    assert!(matches!(p.statements[0].kind, StmtKind::TryCatch { .. }));
}

#[test]
fn try_catch_finally_runs_on_success() {
    // `try` is a statement form (not an expression), like Perl's block syntax.
    let v = run(r#"
        my $x = 0;
        my $r = 0;
        try {
            $r = 10;
        } catch ($err) {
            $r = 0;
        } finally {
            $x = 1;
        }
        $r + $x;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 11);
}

#[test]
fn try_catch_finally_runs_after_catch() {
    let v = run(r#"
        my $x = 0;
        my $r = 0;
        try {
            die "boom";
        } catch ($err) {
            $r = 7;
        } finally {
            $x = 1;
        }
        $r + $x;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 8);
}

#[test]
fn given_when_first_match() {
    let v = run(r#"
        given (7) {
            when (0) { 0; }
            when (7) { 99; }
            default { -1; }
        }
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 99);
}

#[test]
fn given_when_string_eq() {
    let v = run(r#"
        given ("hello") {
            when ("world") { 0; }
            when ("hello") { 1; }
            default { 2; }
        }
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn given_when_regex() {
    let v = run(r#"
        given ("12345") {
            when (/^\d+$/) { 1; }
            default { 0; }
        }
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn eval_timeout_returns_block_value() {
    let v = run("eval_timeout 10 { 3 + 4; }").expect("run");
    assert_eq!(v.to_int(), 7);
}

#[test]
fn eval_timeout_exceeded_errors() {
    let e = run(r#"
        eval_timeout 0 {
            my $i = 0;
            $i = $i + 1 while $i < 999999999;
        }
    "#)
    .expect_err("timeout");
    assert!(
        e.message.contains("eval_timeout") && e.message.contains("exceeded"),
        "{}",
        e.message
    );
}
