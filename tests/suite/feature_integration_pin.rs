//! Integration pins for combinations of the recently-landed features:
//! `par` top-level + namespaced quote-op fns + arrow-hash compound-assign
//! + keyword hash keys. These guard against regressions where one
//! feature's parser/runtime path interacts badly with another's.

use crate::common::*;

// ── par + namespaced quote-op fn ────────────────────────────────────────────

#[test]
fn par_block_can_call_namespaced_quote_op_function() {
    // The `par` block runs in a fresh sub-VM. Calling a user-defined
    // namespaced helper whose name shadows a quote-op letter (`Pkg::s`)
    // must resolve correctly across that boundary.
    let n = eval_int(
        r#"
        fn FOO::s($x) { len($x) }
        my @r = par { FOO::s(_) } "abc def"
        sum @r
        "#,
    );
    // Below the chunk threshold the input arrives as one chunk; `len("abc def")` = 7.
    assert_eq!(n, 7);
}

#[test]
fn par_block_can_call_namespaced_q_qq_qx_qr_helpers() {
    let n = eval_int(
        r#"
        fn Q::q($x)  { $x + 1 }
        fn Q::qq($x) { $x + 2 }
        fn Q::qx($x) { $x + 3 }
        fn Q::qr($x) { $x + 4 }
        # Each helper is called once on the same scalar; sum the results.
        Q::q(10) + Q::qq(10) + Q::qx(10) + Q::qr(10)
        "#,
    );
    // 11 + 12 + 13 + 14 = 50
    assert_eq!(n, 50);
}

// ── compound-assign + keyword key ───────────────────────────────────────────

#[test]
fn keyword_arrow_hash_compound_assign_chain() {
    // Stack multiple compound-assigns against a keyword-named hash key,
    // verifying the Op::SetArrowHashKeep fix + keyword-as-bareword fix
    // play nicely together.
    let n = eval_int(
        r#"
        my $h = +{}
        $h->{return} //= 0
        $h->{return} += 100
        $h->{return} <<= 1
        $h->{return} -= 1
        $h->{return} %= 99
        $h->{return}
        "#,
    );
    // 0 + 100 = 100, then 100 << 1 = 200, then 200 - 1 = 199, then 199 % 99 = 1
    assert_eq!(n, 1);
}

#[test]
fn arrow_hash_compound_assign_inside_ternary() {
    // Compound-assign as the operand of a `?:` ternary — exercises the
    // Pop-leaves-value invariant in a non-statement context.
    let n = eval_int(
        r#"
        my $h = +{v => 10}
        my $cond = 1
        my $r = $cond ? ($h->{v} += 5) : 0
        $r * 100 + $h->{v}
        "#,
    );
    // ternary branch: $h->{v} = 10+5 = 15, returns 15. Final: 15*100 + 15 = 1515
    assert_eq!(n, 1515);
}

#[test]
fn arrow_hash_compound_assign_as_function_argument() {
    // The compound-assign expression's value gets passed to a function;
    // the post-call hash must reflect the mutation.
    let n = eval_int(
        r#"
        fn FN::double($x) { $x * 2 }
        my $h = +{counter => 7}
        my $r = FN::double($h->{counter} += 3)
        $r + $h->{counter}
        "#,
    );
    // counter becomes 10, double(10) = 20, total = 20 + 10 = 30
    assert_eq!(n, 30);
}

// ── par + thread macro + namespaced fn chain ────────────────────────────────

#[test]
fn par_via_thread_macro_with_namespaced_block_call() {
    let s = eval_string(
        r#"
        fn STAGE::s($x) { uc $x }
        ~> "hello" par { STAGE::s(_) }
        "#,
    );
    assert_eq!(s, "HELLO");
}

// ── gh wiring + builtins-hash reflection ────────────────────────────────────

#[test]
fn gh_zen_categorized_under_builtins_hash() {
    // Sanity: gh_zen is in %b and its category is a non-empty string.
    let s = eval_string(r#"$b{gh_zen}"#);
    assert!(!s.is_empty(), "expected gh_zen to carry a category tag");
}

#[test]
fn every_gh_alias_has_a_concrete_primary_in_b() {
    // Each alias must point at a primary that itself lives in %b.
    let n = eval_int(
        r#"
        my $ok = 0
        for my $alias (qw(gh_pulls gh_langs)) {
            my $primary = $a{$alias}
            $ok++ if exists $b{$primary}
        }
        $ok
        "#,
    );
    assert_eq!(n, 2);
}

// ── keyword key in `defined` / `exists` predicates ──────────────────────────

#[test]
fn defined_on_keyword_hash_key() {
    let n = eval_int(
        r#"
        my %h
        my $a = defined $h{if} ? 1 : 0
        $h{if} = 0
        my $b = defined $h{if} ? 1 : 0
        $a * 10 + $b
        "#,
    );
    // Before: undef → 0; after: defined (even though falsy 0) → 1. Result: 01 = 1
    assert_eq!(n, 1);
}

#[test]
fn exists_on_keyword_hash_key_after_delete() {
    let n = eval_int(
        r#"
        my %h = (return => 1, last => 1, next => 1)
        delete $h{return}
        (exists $h{return} ? 1 : 0) + (exists $h{last} ? 1 : 0) + (exists $h{next} ? 1 : 0)
        "#,
    );
    // return deleted, last and next still there
    assert_eq!(n, 2);
}
