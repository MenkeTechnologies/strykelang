//! Algebraic `match (EXPR) { PATTERN => EXPR, ... }` (perlrs extension; tree interpreter).

use perlrs::ast::{ExprKind, MatchPattern, StmtKind};
use perlrs::parse;
use perlrs::run;

#[test]
fn parse_algebraic_match_shape() {
    let p = parse(
        r#"my $x = match ($y) {
        /^\d+$/ => "number",
        _ => "other",
    };"#,
    )
    .expect("parse");
    let stmt = &p.statements[0];
    let StmtKind::My(decls) = &stmt.kind else {
        panic!("expected my");
    };
    let d0 = decls.first().expect("one decl");
    let Some(init) = &d0.initializer else {
        panic!("initializer");
    };
    let ExprKind::AlgebraicMatch { subject, arms } = &init.kind else {
        panic!("expected AlgebraicMatch, got {:?}", init.kind);
    };
    assert!(matches!(subject.kind, ExprKind::ScalarVar(_)));
    assert_eq!(arms.len(), 2);
    assert!(matches!(arms[0].pattern, MatchPattern::Regex { .. }));
    assert!(matches!(arms[1].pattern, MatchPattern::Any));
}

#[test]
fn match_regex_literal_arm() {
    let v = run(r#"my $r = match ("42") {
        /^\d+$/ => "number",
        _ => "other",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "number");
}

#[test]
fn match_array_prefix_and_rest() {
    let v = run(r#"my $a = [1, 2, 9];
    my $r = match ($a) {
        [1, 2, *] => "ok",
        _ => "no",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "ok");
}

#[test]
fn match_hash_capture_and_interpolation() {
    let v = run(r#"my $h = { name => "Alice" };
    my $r = match ($h) {
        { name => $n } => "has name: " . $n,
        _ => "other",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "has name: Alice");
}

#[test]
fn match_non_exhaustive_errors() {
    let e = run(r#"match (1) {
        2 => "two",
    }"#)
    .expect_err("should fail");
    let msg = e.to_string();
    assert!(
        msg.contains("match") || msg.contains("no arm"),
        "unexpected: {}",
        msg
    );
}
