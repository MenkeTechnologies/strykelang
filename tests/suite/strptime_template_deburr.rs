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

// ── unified engine: {{ }} data + <% %> stryke code in one template ────

#[test]
fn template_erb_expr_evaluates() {
    // `<%= expr %>` evaluates a stryke expression into the output.
    assert_eq!(eval_string(r#"template("<%= 2 + 3 %>", {})"#), "5");
}

#[test]
fn template_erb_expr_is_raw_not_html_escaped() {
    // Divergence from stock EJS: `<%=` does NOT auto-escape — `web_h` is
    // explicit. Pins that compat decision so a future change can't silently
    // break existing `<%= web_h($x) %>` views by double-escaping.
    assert_eq!(eval_string(r#"template("<%= '<b>' %>", {})"#), "<b>");
}

#[test]
fn template_erb_code_block_loops() {
    // `<% stmt %>` runs stryke; control flow spans tags (open in one, close
    // in another) around literal text + an `<%= %>` expr. Single-quote the
    // template so the outer interpreter doesn't interpolate `$i` itself.
    assert_eq!(
        eval_string(r#"template('<% for val $i (1:3) { %>[<%= $i %>]<% } %>', {})"#),
        "[1][2][3]",
    );
}

#[test]
fn template_mixes_data_tag_and_code_scalar() {
    // `{{name}}` data tag and `<%= $lang %>` code-scalar (a top-level data
    // key exposed as a scalar) coexist in the same template.
    assert_eq!(
        eval_string(r#"template('Hi {{name}}, <%= $lang %>!', {name => "Ada", lang => "stryke"})"#),
        "Hi Ada, stryke!",
    );
}

#[test]
fn template_erb_comment_is_dropped() {
    assert_eq!(eval_string(r#"template("a<%# ignore %>b", {})"#), "ab");
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
