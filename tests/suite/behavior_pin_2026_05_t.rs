//! Behavior-pinning batch T (2026-05-05): AI Collection Ops, Plotting, Cyberpunk, AI Syntax.
//!
//! This batch pins high-level features including AI batch operators, SVG plotting,
//! and procedural terminal art. It also verifies source-level desugaring for
//! `tool fn` and `mcp_server` constructs.

use crate::common::*;

// ── AI Collection Operators (Mocked) ───────────────────────────────────────

#[test]
fn ai_filter_batches_items_and_filters() {
    // `ai_filter` returns an arrayref of kept items; pass items as an
    // arrayref `[1, 2, 3]` (not a paren-list `(1, 2, 3)` — that flattens
    // into the surrounding arg list and `ai_filter` would see only the
    // first element). Deref result with `@$res`.
    let code = r#"
        ai_mock_install("(?s)criterion.*even", "[false, true, false]");
        my $res = ai_filter([1, 2, 3], "even");
        join(",", @$res)
    "#;
    assert_eq!(eval_string(code), "2");
}

#[test]
fn ai_map_batches_instructions() {
    let code = r#"
        ai_mock_install("(?s)Apply.*double", '["2", "4", "6"]');
        my $res = ai_map([1, 2, 3], "double each number");
        join(",", @$res)
    "#;
    assert_eq!(eval_string(code), "2,4,6");
}

#[test]
fn ai_classify_into_labels() {
    let code = r#"
        ai_mock_install("(?s)Classify.*fruit, vegetable", '["fruit", "vegetable", "fruit"]');
        my $res = ai_classify(["apple", "carrot", "banana"], "", into => ["fruit", "vegetable"]);
        join(",", @$res)
    "#;
    assert_eq!(eval_string(code), "fruit,vegetable,fruit");
}

#[test]
fn ai_sort_by_subjective_criterion() {
    let code = r#"
        ai_mock_install("(?s)Sort.*tastiness", "[2, 0, 1]");
        my $res = ai_sort(["apple", "banana", "cake"], "tastiness");
        join(",", @$res)
    "#;
    assert_eq!(eval_string(code), "cake,apple,banana");
}

#[test]
fn ai_dedupe_groups_indexes() {
    // `ai_dedupe` returns an arrayref containing the first item of each
    // group (one representative per group), not the full groups.
    let code = r#"
        ai_mock_install("(?s)Group duplicates", "[[0, 2], [1]]");
        my $res = ai_dedupe(["apple", "banana", "APPLE"], "case-insensitive");
        join(",", @$res)
    "#;
    assert_eq!(eval_string(code), "apple,banana");
}

#[test]
fn ai_match_boolean_judgment() {
    let code = r#"
        ai_mock_install("(?s)Does this item match.*positive", "true");
        ai_match(42, "is positive")
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── SVG Plotting ───────────────────────────────────────────────────────────

#[test]
fn scatter_svg_generates_circles() {
    let code = r#"scatter_svg([1, 2, 3], [10, 20, 30], "Test Plot")"#;
    let s = eval_string(code);
    assert!(s.contains("<svg"));
    assert!(s.contains("<circle"));
    assert!(s.contains("Test Plot"));
}

#[test]
fn line_svg_generates_polyline() {
    let code = r#"line_svg([1, 2, 3], [10, 20, 30])"#;
    let s = eval_string(code);
    assert!(s.contains("<svg"));
    assert!(s.contains("<polyline"));
}

#[test]
fn pie_svg_generates_slices() {
    let code = r#"pie_svg(+{ Apple => 10, Banana => 20 })"#;
    let s = eval_string(code);
    assert!(s.contains("<svg"));
    assert!(s.contains("Apple"));
    assert!(s.contains("Banana"));
}

#[test]
fn heatmap_svg_generates_rects() {
    let code = r#"heatmap_svg([[1, 2], [3, 4]])"#;
    let s = eval_string(code);
    assert!(s.contains("<svg"));
    assert!(s.contains("<rect"));
}

// ── Cyberpunk Terminal Art ──────────────────────────────────────────────────

#[test]
fn cyber_city_generates_ansi_art() {
    let s = eval_string("cyber_city(40, 10, 123)");
    // Check for ANSI color codes and building characters
    assert!(s.contains("\x1b[38;2;"));
    assert!(s.contains("▄") || s.contains("│") || s.contains("▪"));
}

#[test]
fn cyber_grid_generates_perspective_grid() {
    let s = eval_string("cyber_grid(40, 10)");
    assert!(s.contains("\x1b[38;2;"));
    assert!(s.contains("╱") || s.contains("╲") || s.contains("═"));
}

#[test]
fn cyber_rain_generates_matrix_effect() {
    let s = eval_string("cyber_rain(40, 10)");
    assert!(s.contains("\x1b[38;2;"));
    // Rain characters (Japanese katakana or symbols used in the source)
    assert!(s.contains("ァ") || s.contains("ィ") || s.contains("ゥ") || s.contains("カ") || s.contains(":"));
}

// ── AI Syntax Desugaring (`tool fn` and `mcp_server`) ────────────────────────

#[test]
fn tool_fn_syntax_desugars_and_registers() {
    let code = r#"
        tool fn test_tool($x: int) "A test tool" {
            return $x * 10;
        }
        test_tool(+{ x => 5 })
    "#;
    assert_eq!(eval_int(code), 50);
}

#[test]
fn mcp_server_syntax_desugars_and_starts() {
    // Use AOP to intercept mcp_server_start. Stryke's `around` advice
    // takes a string glob (`"mcp_server_start"`) — the bareword form
    // `around <ident>(args) { ... }` is not supported. Around-advice
    // bodies cannot contain a literal `return` (block lowering rejects
    // it); use the last-expression-is-the-value rule instead.
    let code = r#"
        my $out = "";
        around "mcp_server_start" {
            # AOP advice receives the original call args via `@INTERCEPT_ARGS`,
            # not `@_`. Skip `proceed()` so the real `mcp_server_start` (which
            # would block on a socket) never runs.
            my ($name, $opts) = @INTERCEPT_ARGS;
            $out = "Started $name with " . len($opts->{tools}) . " tools";
            $out
        }
        mcp_server "test_mcp" {
            tool my_tool($a: string) "Doc" { return "hi $a" }
            tool other_tool() "Doc2" { return 42 }
        }
        $out
    "#;
    assert_eq!(eval_string(code), "Started test_mcp with 2 tools");
}

#[test]
fn ai_template_interpolation() {
    // `ai_template` performs `{name}` interpolation only — it does NOT
    // call the AI. Pair it with `ai_prompt` if you want the interpolated
    // string sent to the model.
    let code = r#"ai_template("Hello, {name}!", name => "Alice")"#;
    assert_eq!(eval_string(code), "Hello, Alice!");
}

#[test]
fn ai_summarize_batch() {
    let code = r#"
        ai_mock_install("(?s)Summarize.*long text", "Short summary");
        ai_summarize("This is a very long text that needs summary")
    "#;
    assert_eq!(eval_string(code), "Short summary");
}
