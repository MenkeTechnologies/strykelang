//! `use` / `no` pragmas: strict, warnings, feature.

use crate::common::eval_err_kind;

use forge::error::ErrorKind;
use forge::interpreter::{Interpreter, FEAT_SAY};
use forge::parse;

#[test]
fn use_strict_refs_only() {
    let mut i = Interpreter::new();
    let p = parse("use strict 'refs'; 1").expect("parse");
    i.execute(&p).expect("run");
    assert!(i.strict_refs);
    assert!(!i.strict_subs);
    assert!(!i.strict_vars);
}

#[test]
fn use_strict_default_enables_all_three() {
    let mut i = Interpreter::new();
    let p = parse("use strict; 1").expect("parse");
    i.execute(&p).expect("run");
    assert!(i.strict_refs && i.strict_subs && i.strict_vars);
}

#[test]
fn no_strict_refs_clears_only_refs() {
    let mut i = Interpreter::new();
    let p = parse("use strict; no strict 'refs'; 1").expect("parse");
    i.execute(&p).expect("run");
    assert!(!i.strict_refs);
    assert!(i.strict_subs && i.strict_vars);
}

#[test]
fn no_strict_empty_clears_all() {
    let mut i = Interpreter::new();
    let p = parse("use strict; no strict; 1").expect("parse");
    i.execute(&p).expect("run");
    assert!(!i.strict_refs && !i.strict_subs && !i.strict_vars);
}

#[test]
fn use_feature_qw_say() {
    let mut i = Interpreter::new();
    let p = parse("use feature qw(say); 1").expect("parse");
    i.execute(&p).expect("run");
    assert_ne!(i.feature_bits & FEAT_SAY, 0);
}

#[test]
fn use_feature_bundle_510() {
    let mut i = Interpreter::new();
    let p = parse("use feature ':5.10'; 1").expect("parse");
    i.execute(&p).expect("run");
    assert_ne!(i.feature_bits & FEAT_SAY, 0);
}

#[test]
fn require_strict_enables_strict_like_use() {
    let mut i = Interpreter::new();
    let p = parse("require strict; 1").expect("parse");
    i.execute(&p).expect("run");
    assert!(i.strict_refs && i.strict_subs && i.strict_vars);
}

#[test]
fn strict_refs_rejects_symbolic_scalar_deref() {
    assert_eq!(
        eval_err_kind(r#"use strict; my $foo = "x"; my $x = 1; $$foo"#),
        ErrorKind::Runtime
    );
}

#[test]
fn strict_refs_allows_symbolic_deref_when_refs_off() {
    let mut i = Interpreter::new();
    let p = parse(r#"my $foo = "x"; my $x = 1; $$foo"#).expect("parse");
    let v = i.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn strict_vars_rejects_unqualified_global_read() {
    assert_eq!(
        eval_err_kind("use strict; use strict 'vars'; $xyzzy"),
        ErrorKind::Runtime
    );
}

#[test]
fn strict_subs_hint_on_undefined_sub() {
    let mut i = Interpreter::new();
    let p = parse("use strict; use strict 'subs'; no_such_sub_zzzzzz()").expect("parse");
    let e = i.execute(&p).expect_err("undefined sub");
    assert!(
        e.to_string().contains("strict subs"),
        "expected strict subs hint: {}",
        e
    );
}

#[test]
fn say_requires_feature_when_disabled() {
    assert_eq!(eval_err_kind("no feature 'say'; say 1"), ErrorKind::Runtime);
}
