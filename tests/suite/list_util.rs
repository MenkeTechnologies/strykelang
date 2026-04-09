//! In-tree `vendor/perl/List/Util.pm` (pure Perl; core Perl’s file is XS-based).

use perlrs::interpreter::Interpreter;
use perlrs::value::PerlValue;
use perlrs::{parse, vendor_perl_inc_path};

fn with_vendor_inc() -> Interpreter {
    let mut interp = Interpreter::new();
    let dirs = vec![
        PerlValue::String(vendor_perl_inc_path().to_string_lossy().into_owned()),
        PerlValue::String(".".to_string()),
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
