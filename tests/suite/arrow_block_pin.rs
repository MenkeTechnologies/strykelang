//! Pin `>{ ... }` arrow-block usage inside `~>` thread macros per
//! `docs/STYLE_GUIDE.md` §12: `>{ }` is **only** legal inside
//! `~>` / `~>>` / `~s>` / `~p>` thread macros — it's an
//! arrow-block stage primitive. Probed against the running
//! interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn arrow_block_single_stage_in_thread_macro() {
    // `_` is the threaded value inside `>{}`.
    let code = r#"
        my $r = ~> 5 >{ _ * 2 + 1 };
        $r == 11 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_two_stages_chained() {
    let code = r#"
        my $r = ~> 3 >{ _ * 10 } >{ _ + 7 };
        # 3 -> 30 -> 37
        $r == 37 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_three_stages_independent() {
    let code = r#"
        my $r = ~> 2 >{ _ + 1 } >{ _ * _ } >{ _ - 1 };
        # 2 -> 3 -> 9 -> 8
        $r == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_followed_by_named_fn_stage() {
    let code = r#"
        my $r = ~> 5 >{ _ * 3 } uc;
        $r eq "15" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_with_string_operations() {
    let code = r#"
        my $r = ~> "hello" >{ uc(_) . "!" };
        $r eq "HELLO!" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_does_not_alias_topic() {
    // `_` inside `>{ }` is the threaded value, not the outer `$_`.
    let code = r#"
        $_ = "outer";
        my $r = ~> "inner" >{ _ };
        ($r eq "inner" && $_ eq "outer") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_used_in_thread_last() {
    // `~>>` plus arrow-block: same single-arg semantics since the
    // block sees only the threaded value.
    let code = r#"
        my $r = ~>> 5 >{ _ * 100 };
        $r == 500 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_with_arithmetic_in_thread() {
    let code = r#"
        my $r = ~> 10 >{ _ + 5 } >{ _ * 2 } >{ _ - 1 };
        # 10 -> 15 -> 30 -> 29
        $r == 29 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_then_pipe_forward_exits_thread() {
    // `|>` terminates the `~>` macro and drops back to pipe-forward.
    let code = r#"
        my $r = ~> 7 >{ _ * 6 } |> int;
        $r == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn arrow_block_branching_inside_body() {
    let code = r#"
        my $r = ~> 5 >{ _ > 3 ? _ * 10 : _ };
        $r == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
