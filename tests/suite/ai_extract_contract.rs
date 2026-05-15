//! Pin `ai_extract`'s call-shape contract.
//!
//! The fakery audit found a test author calling
//!   `ai_extract(TEXT, INSTRUCTION, schema => +{...})`
//! — passing a second positional that `parse_opts` then ate as a key,
//! so `schema` was never found and the call died with the "pass schema
//! => +{...}" error. These pins lock the documented one-positional
//! signature so the same hallucination can't ship again.
//!
//! All tests use `with_global_flags` because `ai_mock_install` mutates
//! the process-global mock registry.

use crate::common::*;

// All tests start with `ai_mock_clear()` because the mock registry is
// process-global — a prior test's mocks would leak into the next one
// running in the same `cargo test` invocation.

// ── Documented happy path ─────────────────────────────────────────────

#[test]
fn ai_extract_one_positional_plus_schema_works() {
    let code = r#"
        ai_mock_clear();
        ai_mock_install("(?i)extract user", '{"name":"Alice","age":30}');
        my $r = ai_extract(
            "Alice is 30 years old. Task: extract user",
            schema => +{ name => "string", age => "int" }
        );
        ($r->{name} eq "Alice" && $r->{age} == 30) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

#[test]
fn ai_extract_with_context_opt_appends_to_prompt() {
    let code = r#"
        ai_mock_clear();
        # Mock pattern matches the prompt body — the JSON-only
        # instruction footer plus the user-provided context block
        # will both appear, so anchor on the original text.
        ai_mock_install("(?s)extract user.*Context", '{"name":"Bob","age":25}');
        my $r = ai_extract(
            "extract user",
            schema  => +{ name => "string", age => "int" },
            context => "Bob is 25 and lives in Atlanta."
        );
        ($r->{name} eq "Bob" && $r->{age} == 25) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

#[test]
fn ai_extract_missing_schema_errors_with_helpful_message() {
    // No `schema =>` opt → must die with the canonical "pass schema =>
    // +{field => type}" error. Pinning the error message keeps it from
    // drifting into something less actionable.
    let code = r#"
        ai_mock_clear();
        my $err = "";
        eval {
            ai_extract("anything");
        };
        $err = "$@" if $@;
        ($err =~ /pass schema/) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

// ── Fakery-style call shapes must fail loudly ─────────────────────────

#[test]
fn ai_extract_with_extra_positional_errors_not_silently_loses_schema() {
    // The Gemini-style fakery: passing an `instruction` as a second
    // positional. With the strict one-positional contract, the second
    // positional gets read as an opt key by `parse_opts`, the
    // `schema` arg pair gets shifted by one, and the error fires.
    // This pins the "loud failure" behavior — silent garbage would be
    // worse than the runtime error.
    let code = r#"
        ai_mock_clear();
        my $err = "";
        eval {
            # WRONG shape (two positionals + opts) — must not silently
            # succeed with stripped opts.
            my $r = ai_extract(
                "Some text to parse",
                "extract user",
                schema => +{ name => "string" }
            );
        };
        $err = "$@" if $@;
        ($err =~ /pass schema/) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

#[test]
fn ai_extract_no_args_errors_on_missing_schema() {
    // `ai_extract()` with zero args: stryke passes an empty arg slice,
    // `args.first()` returns None → prompt defaults to "" via the
    // existing impl, then parse_opts gives empty → the schema-required
    // check fires. Pins the actual error path (the prompt-required
    // branch is unreachable in current impl since first() of empty
    // returns None and the .map().ok_or_else() chain only triggers
    // "prompt required" when args.first() is None AND maps to None).
    let code = r#"
        ai_mock_clear();
        my $err = "";
        eval {
            ai_extract();
        };
        $err = "$@" if $@;
        ($err =~ /pass schema|prompt required/) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

// ── Reflection: ai_extract is in %b and %d ────────────────────────────

#[test]
fn ai_extract_is_documented_in_d_hash() {
    let code = r#"
        my $doc = $d{"ai_extract"};
        (defined($doc) && length($doc) > 10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
