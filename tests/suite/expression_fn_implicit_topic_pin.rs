//! Pin the **no-param** expression-bodied `fn` form from
//! `docs/STYLE_GUIDE.md` rule 6: `fn name = _ * 2`. Existing
//! `expression_bodied_fn.rs` covers `fn name(PARAMS) = EXPR`;
//! this file covers the implicit-topic form (`_`, `_0`, `_1`, …).
//! Probed against the running interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn implicit_topic_single_arg() {
    let code = r#"
        fn Demo::Eft::dbl = _ * 2;
        Demo::Eft::dbl(21)
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn implicit_topic_squared() {
    let code = r#"
        fn Demo::Eft::sq = _ * _;
        Demo::Eft::sq(7)
    "#;
    assert_eq!(eval_int(code), 49);
}

#[test]
fn implicit_two_positional_args() {
    let code = r#"
        fn Demo::Eft::sum2 = _0 + _1;
        Demo::Eft::sum2(10, 20)
    "#;
    assert_eq!(eval_int(code), 30);
}

#[test]
fn implicit_three_positional_args() {
    let code = r#"
        fn Demo::Eft::sum3 = _0 + _1 + _2;
        Demo::Eft::sum3(1, 2, 3)
    "#;
    assert_eq!(eval_int(code), 6);
}

#[test]
fn implicit_topic_with_string_arg() {
    let code = r#"
        fn Demo::Eft::shout = uc(_) . "!";
        Demo::Eft::shout("hi") eq "HI!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn implicit_topic_with_pipe_forward_body() {
    let code = r#"
        fn Demo::Eft::clean = _ |> trim |> uc;
        Demo::Eft::clean("  hello  ") eq "HELLO" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn implicit_topic_paren_less_call() {
    // Per style guide §0a: list-operator-style paren-less call.
    let code = r#"
        fn Demo::Eft::dbl = _ * 2;
        my $r = Demo::Eft::dbl 21;
        $r
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn implicit_multi_arg_paren_less_call() {
    let code = r#"
        fn Demo::Eft::sum2 = _0 + _1;
        my $r = Demo::Eft::sum2 10, 20;
        $r
    "#;
    assert_eq!(eval_int(code), 30);
}

#[test]
fn implicit_topic_with_ternary_body() {
    let code = r#"
        fn Demo::Eft::abs_int = _ < 0 ? -_ : _;
        my $a = Demo::Eft::abs_int(-7);
        my $b = Demo::Eft::abs_int(5);
        ($a == 7 && $b == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn implicit_topic_in_map_chain_via_pipe_forward() {
    let code = r#"
        fn Demo::Eft::cube = _ * _ * _;
        my $sum = (1..5) |> map { Demo::Eft::cube(_) } |> sum;
        # 1 + 8 + 27 + 64 + 125 = 225
        $sum
    "#;
    assert_eq!(eval_int(code), 225);
}

#[test]
fn implicit_topic_recursive_fn() {
    let code = r#"
        fn Demo::Eft::fact = _ <= 1 ? 1 : _ * Demo::Eft::fact(_ - 1);
        Demo::Eft::fact(6)
    "#;
    assert_eq!(eval_int(code), 720);
}

#[test]
fn implicit_topic_with_pipe_forward_arg_threading() {
    // 5 |> Demo::dbl  →  Demo::dbl(5) = 10
    let code = r#"
        fn Demo::Eft::dbl = _ * 2;
        5 |> Demo::Eft::dbl
    "#;
    assert_eq!(eval_int(code), 10);
}
