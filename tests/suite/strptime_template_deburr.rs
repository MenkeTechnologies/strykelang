//! `strptime` (arbitrary-format date parse), `template` (runtime Mustache-subset
//! rendering), and `deburr` (Latin diacritic folding) builtins.

use crate::common::*;

// ── strptime ─────────────────────────────────────────────────────────

#[test]
fn strptime_parses_custom_format_to_epoch() {
    // 2026-06-15 00:00:00 UTC == 1781481600.
    assert_eq!(eval_string(r#"strptime("2026-06-15", "%Y-%m-%d")"#), "1781481600");
}

#[test]
fn strptime_parses_log_style_stamp() {
    // Apache/nginx-style stamp; verify the field decode round-trips through strftime.
    assert_eq!(
        eval_string(
            r#"datetime_strftime(strptime("15/Jun/2026:14:30:00", "%d/%b/%Y:%H:%M:%S"), "%Y-%m-%dT%H:%M:%S")"#
        ),
        "2026-06-15T14:30:00",
    );
}

#[test]
fn strptime_agrees_with_parse_local_for_utc() {
    // strptime interprets as UTC; datetime_parse_local with UTC zone must match.
    assert_eq!(
        eval_string(
            r#"strptime("2026-06-15 14:30:00", "%Y-%m-%d %H:%M:%S") == datetime_parse_local("2026-06-15 14:30:00", "UTC") ? "ok" : "no""#
        ),
        "ok",
    );
}

// ── template ─────────────────────────────────────────────────────────

#[test]
fn template_interpolates_variable() {
    assert_eq!(
        eval_string(r#"template("Hi {{name}}!", {name => "Ada"})"#),
        "Hi Ada!",
    );
}

#[test]
fn template_missing_var_is_empty() {
    assert_eq!(eval_string(r#"template("[{{x}}]", {})"#), "[]");
}

#[test]
fn template_section_iterates_array_with_dot() {
    assert_eq!(
        eval_string(r#"template("{{#xs}}-{{.}} {{/xs}}", {xs => ["a", "b", "c"]})"#),
        "-a -b -c ",
    );
}

#[test]
fn template_section_over_hashref_scopes_keys() {
    assert_eq!(
        eval_string(r#"template("{{#u}}{{first}} {{last}}{{/u}}", {u => {first => "Grace", last => "Hopper"}})"#),
        "Grace Hopper",
    );
}

#[test]
fn template_inverted_renders_on_empty() {
    assert_eq!(
        eval_string(r#"template("{{^xs}}none{{/xs}}", {xs => []})"#),
        "none",
    );
}

#[test]
fn template_inverted_skips_when_present() {
    assert_eq!(
        eval_string(r#"template("{{^xs}}none{{/xs}}", {xs => [1]})"#),
        "",
    );
}

#[test]
fn template_comment_is_dropped() {
    assert_eq!(
        eval_string(r#"template("a{{! ignore me }}b", {})"#),
        "ab",
    );
}

#[test]
fn template_nested_sections() {
    assert_eq!(
        eval_string(
            r#"template("{{#rows}}[{{#cells}}{{.}}{{/cells}}]{{/rows}}", {rows => [{cells => ["a","b"]}, {cells => ["c"]}]})"#
        ),
        "[ab][c]",
    );
}

// ── deburr ───────────────────────────────────────────────────────────

#[test]
fn deburr_folds_basic_accents() {
    assert_eq!(eval_string(r#"deburr("déjà vu")"#), "deja vu");
}

#[test]
fn deburr_folds_creme_brulee() {
    assert_eq!(eval_string(r#"deburr("Crème brûlée")"#), "Creme brulee");
}

#[test]
fn deburr_expands_ligatures_and_sharp_s() {
    // Æ→Ae, œ→oe, ß→ss, Þ→Th.
    assert_eq!(eval_string(r#"deburr("Æsop œuvre straße Þor")"#), "Aesop oeuvre strasse Thor");
}

#[test]
fn deburr_leaves_ascii_untouched() {
    assert_eq!(eval_string(r#"deburr("plain ASCII 123")"#), "plain ASCII 123");
}

#[test]
fn deburr_strips_combining_marks() {
    // "e" + U+0301 (combining acute) collapses to "e".
    assert_eq!(eval_string("deburr(\"e\u{0301}\")"), "e");
}
