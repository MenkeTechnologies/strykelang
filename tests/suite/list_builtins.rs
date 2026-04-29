//! Tests for stryke list builtins (sum, min, max, uniq, reduce, zip, pairs, ...)
//! for `%INC` / `require`. Use `Interpreter::new()` so subs are registered (tests may add `vendor/perl` to `@INC`).

use stryke::interpreter::Interpreter;
use stryke::value::PerlValue;
use stryke::{parse, vendor_perl_inc_path};

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
fn bare_builtin_uniq_adjacent_dedup() {
    let mut interp = with_vendor_inc();
    let p = parse("(1,1,2,3) |> uniq |> join \",\"").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "1,2,3");
}

#[test]
fn bare_builtin_sum_and_sum0() {
    let mut interp = with_vendor_inc();
    let p = parse("sum(1,2,3) + sum0()").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 6);
}

#[test]
fn bare_builtin_min_max() {
    let mut interp = with_vendor_inc();
    let p =
        parse("(min(3,9,2), max(3,9,2), minstr(\"b\",\"a\"), maxstr(\"b\",\"a\")) |> join \",\"")
            .expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "2,9,a,b");
}

#[test]
fn bare_builtin_require_loads_pm() {
    let mut interp = with_vendor_inc();
    let p = parse("(7,7,8) |> uniq |> join \",\"").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "7,8");
}

#[test]
fn bare_builtin_reduce_block_form() {
    let mut interp = with_vendor_inc();
    let p = parse("(1, 2, 3, 4) |> reduce { $a + $b }").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 10);
}

#[test]
fn bare_builtin_any_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("any(fn { $_ > 2 }, 1, 2, 3)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn bare_builtin_all_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("all(fn { $_ > 0 }, 1, 2, 3)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn bare_builtin_none_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("none(fn { $_ < 0 }, 1, 2, 3)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn bare_builtin_notall_coderef() {
    let mut interp = Interpreter::new();
    let p = parse("notall(fn { $_ > 0 }, 1, -1, 2)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn bare_builtin_product_scalar() {
    let mut interp = Interpreter::new();
    let p = parse("product(2, 3, 4)").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 24);
}

#[test]
fn bare_builtin_sum_in_list_context_joins_like_scalar() {
    let mut interp = Interpreter::new();
    let p = parse(r#"sum(10, 3) |> join ','"#).expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "13");
}

#[test]
fn bare_sum_min_max_product_without_import() {
    let mut interp = Interpreter::new();
    let p = parse(r#"min(9, 2) + max(1, 4) + sum(1, 2) + product(2, 3)"#).expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 15);
}

#[test]
fn bare_mean_and_mode_stats() {
    let mut interp = Interpreter::new();
    let p = parse(r#"mean(2, 4, 10) == mean(2, 4, 10)"#).expect("parse");
    let v = interp.execute(&p).expect("run");
    assert!(v.is_true());
    let p2 = parse(r#"my @m = mode(1, 2, 2, 3, 3); len(@m) + $m[0] + $m[1]"#).expect("parse");
    let v2 = interp.execute(&p2).expect("run");
    assert_eq!(v2.to_int(), 2 + 2 + 3);
}

#[test]
fn bare_builtin_uniqstr_case_sensitive() {
    let mut interp = Interpreter::new();
    let p = parse(r#"uniqstr("a", "A", "a") |> join ','"#).expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "a,A,a");
}

#[test]
fn bare_builtin_mesh_interleaves_array_refs() {
    let mut interp = Interpreter::new();
    let p = parse(r#"my @m = mesh([1, 2], [10, 20]); @m |> join ','"#).expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_string(), "1,10,2,20");
}

#[test]
fn bare_builtin_zip_shortest_pairs_rows_by_min_length() {
    let mut interp = Interpreter::new();
    let p = parse(
        "my @z = zip_shortest([1, 2, 3], [10, 20]); len(@z) + $z[0]->[0] + $z[0]->[1] + $z[1]->[0] + $z[1]->[1]",
    )
    .expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), 2 + 1 + 10 + 2 + 20);
}

#[test]
fn bare_builtin_zip_longest_pairs_all_rows_shorter_list_pads_second_column() {
    let mut interp = Interpreter::new();
    let p = parse(
        "my @z = zip_longest([1, 2], [10]); len(@z) + $z[0]->[0] + $z[0]->[1] + $z[1]->[0] + ($z[1]->[1] + 0)",
    )
    .expect("parse");
    let v = interp.execute(&p).expect("run");
    assert_eq!(v.to_int(), (2 + 1 + 10 + 2));
}

#[test]
fn bare_builtin_pairs_returns_blessed_arrays() {
    let mut interp = Interpreter::new();
    let p = parse("pairs(\"a\", 1, \"b\", 2) |> join \",\"").expect("parse");
    let v = interp.execute(&p).expect("run");
    assert!(v.to_string().len() >= 4);
}
