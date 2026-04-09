//! `use` / `no` pragmas: strict, warnings, feature.

use perlrs::interpreter::{Interpreter, FEAT_SAY};
use perlrs::parse;

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
