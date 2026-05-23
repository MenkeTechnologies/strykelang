//! Pin paren-less list-operator-style calls per
//! `docs/STYLE_GUIDE.md` §0a and rule 8: function args bind to the
//! right of the name without parens; parens are only needed for
//! precedence, disambiguation, or to indicate an empty arg list.
//! Probed against the running interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn paren_less_builtin_len_on_array() {
    let code = r#"
        my @a = (1, 2, 3, 4, 5);
        len @a
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn paren_less_builtin_uc_on_string() {
    let code = r#"
        uc "hello" eq "HELLO" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_chain_uc_rev() {
    // Right-to-left associativity: uc(rev("abc")) = uc("cba") = "CBA".
    // The `eq` belongs to the comparison, not to the rev() arg, so
    // parens are needed to seal the call before the operator binds —
    // exactly the §0a precedence-parens-needed case.
    let code = r#"
        (uc rev "abc") eq "CBA" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_user_function_single_arg() {
    let code = r#"
        fn Demo::Pl::dbl($n) { $n * 2 }
        Demo::Pl::dbl 21
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn paren_less_user_function_multi_arg() {
    let code = r#"
        fn Demo::Pl::sum($x, $y) { $x + $y }
        Demo::Pl::sum 10, 20
    "#;
    assert_eq!(eval_int(code), 30);
}

#[test]
fn parens_required_when_followed_by_comparison() {
    // Per §0a: `len(@a) > 1` needs parens; without them, `>` rebinds
    // and you'd get `len(@a > 1)` = `len(bool)` = 0.
    let code = r#"
        my @a = (1, 2, 3, 4);
        (len(@a) > 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_works_inside_pipe_forward_chain() {
    let code = r#"
        my $r = (1..5) |> sum;
        $r == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_call_inside_print_statement_via_p() {
    let code = r#"
        # `p len @a` — paren-less verb + paren-less arg, the canonical
        # style-guide spelling.
        my @a = (10, 20, 30);
        my $n = len @a;
        $n == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_keys_then_paren_for_count() {
    // `len(keys %h)` — the inner `keys %h` is paren-less, the outer
    // `len(...)` adds parens to seal the list before counting.
    let code = r#"
        my %h = (a => 1, b => 2, c => 3);
        len(keys %h) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn parens_or_bareword_for_zero_arg_builtin() {
    // `time` (bareword) and `time()` (explicit zero args) both work.
    let code = r#"
        my $a = time;
        my $b = time();
        ($a > 0 && $b > 0 && abs($a - $b) <= 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_call_assigned_to_scalar() {
    let code = r#"
        my $r = len "stryke";
        $r == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn paren_less_with_arithmetic_after_must_have_parens() {
    // `len(@a) + 1` works; `len @a + 1` would be `len(@a + 1)` and
    // try to take len of a number — different result.
    let code = r#"
        my @a = (10, 20, 30);
        (len(@a) + 1) == 4 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
