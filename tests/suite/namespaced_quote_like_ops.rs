//! Pin tests for namespaced quote-like operators — `Foo::s`, `Foo::m`,
//! `Foo::q`, `Foo::qq`, `Foo::qx`, `Foo::qr` (joining the existing
//! `Foo::tr` / `Foo::y` coverage in `regression_2026_04.rs`).
//!
//! Pre-fix the lexer would still eat the quote-like body after the `::`
//! (e.g. `fn Foo::s($n, $k) = ...` was parsed as `fn Foo::s/...../$k=.../...`
//! and surfaced as a syntax error). After fix, anything appearing as an
//! identifier after `::` is treated as a plain name, never a quote op.

use crate::common::*;
use stryke::error::ErrorKind;

// ── definitional form: `fn Pkg::OP($x) { ... }` parses & runs ────────────────

#[test]
fn fn_def_named_s_under_namespace_runs() {
    let n = eval_int(
        r#"
        fn Foo::s($n) { $n + 100 }
        Foo::s(5)
        "#,
    );
    assert_eq!(n, 105);
}

#[test]
fn fn_def_named_m_under_namespace_runs() {
    let n = eval_int(
        r#"
        fn Foo::m($n) { $n * 2 }
        Foo::m(7)
        "#,
    );
    assert_eq!(n, 14);
}

#[test]
fn fn_def_named_q_qq_qx_qr_under_namespace_runs() {
    // All four short quote-like operators must be callable as namespaced
    // function names with a body, not just sub-ref form.
    assert_eq!(
        eval_int(r#"fn Foo::q($n) { $n + 1 } Foo::q(10)"#),
        11
    );
    assert_eq!(
        eval_int(r#"fn Foo::qq($n) { $n + 2 } Foo::qq(10)"#),
        12
    );
    assert_eq!(
        eval_int(r#"fn Foo::qx($n) { $n + 3 } Foo::qx(10)"#),
        13
    );
    assert_eq!(
        eval_int(r#"fn Foo::qr($n) { $n + 4 } Foo::qr(10)"#),
        14
    );
}

#[test]
fn deep_namespace_named_s_runs() {
    let n = eval_int(
        r#"
        fn Outer::Inner::s($n, $k) { $n * $k }
        Outer::Inner::s(6, 7)
        "#,
    );
    assert_eq!(n, 42);
}

// ── sub-ref form: \&Pkg::OP doesn't try to lex a quote body ──────────────────

#[test]
fn sub_ref_to_namespaced_m_does_not_lex_match_body() {
    // `\&Foo::m` would previously try to consume an `m//` body. After fix it
    // resolves to an undefined-sub at runtime, which is the correct error.
    let kind = eval_err_kind(r#"my $f = \&Foo::m; $f->()"#);
    assert_eq!(kind, ErrorKind::Runtime);
}

#[test]
fn sub_ref_to_namespaced_q_qq_qx_qr_resolves_to_undefined_sub() {
    for name in ["q", "qq", "qx", "qr"] {
        let code = format!(r#"my $f = \&Foo::{}; $f->()"#, name);
        let kind = eval_err_kind(&code);
        assert_eq!(
            kind,
            ErrorKind::Runtime,
            "expected runtime undefined-sub for Foo::{}",
            name
        );
    }
}

// ── inside-package definition: `fn Pkg::s` survives lexer fold ──────────────

#[test]
fn nested_namespace_with_s_and_m_can_coexist() {
    // Define both `Foo::s` and `Foo::m` in the same translation unit; both
    // must round-trip. Previously the `s/.../.../` lexer arm would have
    // swallowed the next `m` body.
    let total = eval_int(
        r#"
        fn Foo::s($n) { $n * 3 }
        fn Foo::m($n) { $n + 5 }
        Foo::s(2) + Foo::m(2)
        "#,
    );
    assert_eq!(total, 6 + 7);
}
