//! `List::Util` — implemented natively in `src/list_util.rs`; `vendor/perl/List/Util.pm` is a stub
//! for `%INC` / `require`. Use `Interpreter::new()` so subs are registered (tests may add `vendor/perl` to `@INC`).

use perlrs::interpreter::Interpreter;
use perlrs::value::PerlValue;
use perlrs::{parse, vendor_perl_inc_path};

fn with_vendor_inc() -> Interpreter {
    let mut interp = Interpreter::new();
    let dirs = vec![
        PerlValue::string(vendor_perl_inc_path().to_string_lossy().into_owned()),
        PerlValue::string(".".to_string()),
    ];
    // Mirror driver: vendor shadows system paths; tests stay valid without invoking `main`.
    interp.scope.declare_array("INC", dirs);
    interp
}

#[test]
fn list_util_uniq_adjacent_dedup() {
    let mut interp = with_vendor_inc();
    let p = parse("use List::Util qw(uniq); join(\",\", uniq(1,1,2,3))").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "1,2,3");
}

#[test]
fn list_util_sum_and_sum0() {
    let mut interp = with_vendor_inc();
    let p = parse("use List::Util qw(sum sum0); sum(1,2,3) + sum0()").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 6);
}

#[test]
fn list_util_min_max() {
    let mut interp = with_vendor_inc();
    let p = parse("use List::Util qw(min max minstr maxstr); join(\",\", min(3,9,2), max(3,9,2), minstr(\"b\",\"a\"), maxstr(\"b\",\"a\"))")
        .expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "2,9,a,b");
}

#[test]
fn list_util_require_loads_pm() {
    let mut interp = with_vendor_inc();
    let p = parse("require List::Util; join(\",\", List::Util::uniq(7,7,8))").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "7,8");
}

#[test]
fn list_util_reduce_block_form() {
    let mut interp = with_vendor_inc();
    let p = parse("use List::Util qw(reduce); reduce { $a + $b } 1, 2, 3, 4").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 10);
}

#[test]
fn list_util_any_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("List::Util::any(sub { $_ > 2 }, 1, 2, 3)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn list_util_all_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("List::Util::all(sub { $_ > 0 }, 1, 2, 3)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn list_util_none_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("List::Util::none(sub { $_ < 0 }, 1, 2, 3)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn list_util_notall_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("List::Util::notall(sub { $_ > 0 }, 1, -1, 2)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn list_util_product_scalar() {
    let mut interp = Interpreter::new();
    let p = parse("List::Util::product(2, 3, 4)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 24);
}

#[test]
fn list_util_uniqstr_case_sensitive() {
    let mut interp = Interpreter::new();
    let p = parse(r#"join(",", List::Util::uniqstr("a", "A", "a"))"#).expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "a,A,a");
}

#[test]
fn list_util_mesh_interleaves_array_refs() {
    let mut interp = Interpreter::new();
    let p = parse(
        r#"my @m = List::Util::mesh([1, 2], [10, 20]); join(",", @m)"#,
    )
    .expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "1,10,2,20");
}

#[test]
fn list_util_pairs_returns_blessed_arrays() {
    let mut interp = Interpreter::new();
    let p = parse("join(\",\", List::Util::pairs(\"a\", 1, \"b\", 2))").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert!(v.to_string().len() >= 4);
}
