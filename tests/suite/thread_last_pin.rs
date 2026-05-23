//! Pin thread-last `->>` and `~>>` macros per
//! `docs/STYLE_GUIDE.md` §7.3: same syntax as `~>` but the threaded
//! value goes in as the **last** argument of each stage. Useful for
//! Perl-tradition list-consumers like `map`/`grep`/`reduce` whose
//! arity is `(block, list)`. Probed against the running interpreter
//! on 2026-05-23.
//!
//! `~>` thread-first is heavily covered by `threading_macro_pin.rs`;
//! this file covers the thread-last variants, which were 0%-pinned
//! before.

use crate::common::*;

#[test]
fn thread_last_two_arg_fn_puts_value_in_last_slot() {
    // div(2, 10) = 2/10 = 0.2 — proves the threaded value (10) is
    // the LAST arg.
    let code = r#"
        fn Demo::Tl::div = _0 / _1;
        my $r = ->> 10 Demo::Tl::div(2);
        ($r > 0.199 && $r < 0.201) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_last_explicit_placeholder_overrides_position() {
    // With explicit `_`, the threaded value goes wherever `_`
    // appears. div(10, 2) = 5.
    let code = r#"
        fn Demo::Tl::div = _0 / _1;
        my $r = ->> 10 Demo::Tl::div(_, 2);
        $r == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_last_subtract_demonstrates_position() {
    // sub(3, 10) = -7  vs  sub(10, 3) = 7 — opposite results
    // depending on which slot gets the threaded value.
    let code = r#"
        fn Demo::Tl::sub = _0 - _1;
        my $r = ->> 10 Demo::Tl::sub(3);
        $r == -7 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_last_chained_two_stages() {
    let code = r#"
        fn Demo::Tl::sub = _0 - _1;
        my $r = ->> 100 Demo::Tl::sub(10) Demo::Tl::sub(3);
        # 100 → sub(10, 100) = -90 → sub(3, -90) = 93
        $r == 93 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tilde_thread_last_two_arg_fn() {
    // The `~>>` spelling threads the same way.
    let code = r#"
        fn Demo::Tl::div = _0 / _1;
        my $r = ~>> 10 Demo::Tl::div(2);
        ($r > 0.199 && $r < 0.201) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tilde_thread_last_with_block_arg_form() {
    // The Perl-tradition `map { … } LIST` shape is exactly the
    // shape thread-last targets: the list goes last.
    let code = r#"
        my @r = ~>> (1, 2, 3, 4, 5) map { _ * _ };
        join(",", @r) eq "1,4,9,16,25" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tilde_thread_last_with_grep_block() {
    let code = r#"
        my @r = ~>> (1..10) grep { _ > 5 };
        join(",", @r) eq "6,7,8,9,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn tilde_thread_last_chained_map_then_grep() {
    let code = r#"
        my @r = ~>> (1..6) map { _ * _ } grep { _ > 10 };
        join(",", @r) eq "16,25,36" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_last_single_stage_is_just_call() {
    let code = r#"
        fn Demo::Tl::neg = -_;
        my $r = ->> 5 Demo::Tl::neg;
        $r == -5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_last_arithmetic_difference_from_thread_first() {
    // Same call site with `->>` vs `~>` should give different
    // results for any non-commutative op — pins the position
    // contract.
    let code = r#"
        fn Demo::Tl::sub = _0 - _1;
        my $last  = ->> 10 Demo::Tl::sub(3);   # sub(3, 10) = -7
        my $first = ~>  10 Demo::Tl::sub(3);   # sub(10, 3) =  7
        ($last == -7 && $first == 7) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
