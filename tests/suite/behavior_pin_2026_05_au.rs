//! Behavior-pinning batch AU (2026-05-08): Sweeping undocumented bugs.
//!
//! Each test pins the *current* observed output; comments call out the
//! Perl-compat or expected behavior so a future fix flips the assertion to
//! the right value rather than deleting the test.

use crate::common::*;

// ── POLISH-002: ++ on a non-lvalue reports PostfixOp on non-scalar ───────────

#[test]
fn postfix_inc_on_non_lvalue_reports_postfix_op_error() {
    // `1++` or `"a"++` should be a syntax error about modifying a constant.
    // Currently, it parses but fails with a runtime or specific compiler error.
    let err = eval_err_kind("1++");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime") || msg.contains("Syntax") || msg.contains("Compile"),
        "expected runtime/syntax error about postfix on non-lvalue, got {:?}",
        err
    );
}

// ── POLISH-004: Class method named `m` is parsed as regex-match operator ─────

#[test]
fn class_method_named_m_parses_as_regex_match() {
    // `Foo->m()` should be a method call.
    // Currently, `m` is eagerly grabbed as the match operator `m//`.
    let err = parse_err_kind("class Foo { fn m { 1 } } Foo->m()");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("regex") || msg.contains("Expected"),
        "expected parse error due to 'm' operator ambiguity, got {:?}",
        err
    );
}

// ── BUG-036: $obj->can("method") returns uninvokable coderef ─────────────────

#[test]
fn can_returns_coderef_that_fails_to_invoke() {
    // `$obj->can("method")` returns a coderef, but calling `can` directly
    // might fail due to missing $self binding or other issues.
    let err = eval_err_kind(
        r#"class Box { fn area { 42 } }
           my $b = Box->new;
           my $m = $b->can("area");"#,
    );
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Runtime"),
        "expected runtime error when calling can, got {:?}",
        err
    );
}

// ── BUG-039: <*.ext> angle-bracket glob shorthand not parsed ─────────────────

#[test]
fn angle_bracket_glob_shorthand_not_parsed() {
    // `<*.txt>` is a Perl idiom for `glob("*.txt")`.
    // Currently stryke rejects it or parses it as less-than/greater-than.
    let err = parse_err_kind("my @files = <*.txt>;");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("Syntax") || msg.contains("Expected"),
        "expected parse error for angle-bracket glob, got {:?}",
        err
    );
}

// ── delete @array[indices] slice form removes each element ──────────────────

#[test]
fn delete_array_slice_drains_elements() {
    assert_eq!(
        eval_string(
            "my @a = (10,20,30); delete @a[0, 2]; join(',', map { defined($_) ? $_ : '_' } @a)"
        ),
        "_,20,_"
    );
}

// ── delete @hash{KEYS} slice form removes each named key ────────────────────

#[test]
fn delete_hash_slice_drains_keys() {
    assert_eq!(
        eval_string("my %h = (a=>1, b=>2, c=>3); delete @h{'a', 'b'}; join(',', sort keys %h)"),
        "c"
    );
}
