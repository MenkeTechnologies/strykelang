//! Regression pins for library fixes landed during the 2026-05-15 session.
//!
//! Each test asserts the *fixed* behavior; if any of these flips back,
//! a regression slipped past code review. Annotations point at the
//! commit-time impl location so the failure points at the right file.

use crate::common::*;

// ── BUG-126/140: uniq derefs single arrayref args ─────────────────────
//
// Fix: `strykelang/list_builtins.rs::uniq_list` — added an
// `as_array_ref` branch so `uniq([1,1,2,2])` unfolds to (1,1,2,2)
// instead of treating the arrayref as a single atom.

#[test]
fn uniq_arrayref_literal_derefs() {
    // Before fix: returned [arrayref] (len 1). After fix: [1, 2] (len 2).
    let code = r#"
        my @r = uniq([1, 1, 2, 2]);
        join(",", @r) eq "1,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uniq_arrayref_to_named_array() {
    let code = r#"
        my @data = (3, 3, 1, 1, 2, 2);
        my @r = uniq(\@data);
        join(",", @r) eq "3,1,2" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uniq_mixed_args_flatten_arrayref_in_place() {
    // arrayref `[10, 10, 20]` should unfold inside the variadic stream.
    let code = r#"
        my @r = uniq("a", "a", [10, 10, 20], "b");
        join(",", @r) eq "a,10,20,b" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uniq_variadic_unchanged() {
    // Pre-fix behavior on flat args must not regress.
    let code = r#"
        my @r = uniq(1, 1, 2, 2, 3);
        join(",", @r) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uniq_empty_arrayref_yields_empty() {
    let code = r#"
        my @r = uniq([]);
        scalar(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn uniq_nested_arrayref_recurses_one_level_only() {
    // Outer arrayref is dereffed once into 3 inner arrayrefs. Inner
    // arrayrefs are then compared by string form — and stryke
    // stringifies every arrayref as `ARRAY(0x...)`, so all three inner
    // refs collide into a single uniq atom. This matches Perl 5
    // `List::Util::uniq` behavior on refs.
    let code = r#"
        my @r = uniq([[1, 2], [1, 2], [3, 4]]);
        scalar(@r) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ai_sugar.rs: newline as statement boundary in tool-fn desugar ─────
//
// Fix: `strykelang/ai_sugar.rs::desugar_tool_fn` + `desugar_mcp_server`
// — the `b'\n'` arm now sets `can_start_stmt = true`. Previously any
// line that didn't end in `;` `{` `}` left `can_start_stmt = false`,
// causing a top-of-line `tool fn` on the next line to be silently
// skipped by the desugarer and surface as a raw parser error.

#[test]
fn tool_fn_after_string_literal_line_compiles() {
    // The minimal repro: an arbitrary statement that ends in a string
    // literal (no trailing `;`) followed by `tool fn`. Pre-fix this
    // failed with `Expected LBrace, got Ident("get_temp")`.
    let code = r#"
        my $msg = "hello"
        tool fn get_temp($city: string) "doc" { 99 }
        get_temp(+{ city => "SF" })
    "#;
    assert_eq!(eval_int(code), 99);
}

#[test]
fn tool_fn_after_pipe_aeq_compiles() {
    // The shape that originally surfaced the bug: `|> aeq EXPR, "msg"`
    // followed by `tool fn`. The trailing string used to confuse the
    // parser; the desugar's newline reset now treats line boundary
    // correctly.
    let code = r#"
        my $x = 1
        $x |> aeq 1, "msg"
        tool fn calc($n: int) "doc" { $n * 10 }
        calc(+{ n => 5 })
    "#;
    assert_eq!(eval_int(code), 50);
}

// ── ai_session_history: system row prefix + flat-array return shape ──
//
// Fix: `strykelang/ai.rs::ai_session_history` — prepends a `{role:
// "system", content: ...}` row when the session has a system prompt,
// and returns `StrykeValue::array(...)` (flat) instead of `array_ref`
// (1-element wrapper). Matches the convention of `ai_memory_recall`.

#[test]
fn ai_session_history_includes_system_row_first() {
    let code = r#"
        # Activate mock-embed mode via the sentinel mock — `mock_embed_active`
        # checks `match_mock("embed:probe")`, so any mock whose pattern
        # matches that literal counts.
        ai_mock_install("embed:probe", "");
        ai_mock_install('(?i)\bhi\b', "Hi back");
        my $s = ai_session_new(system => "You are helpful");
        ai_session_send($s, "hi");
        my @h = ai_session_history($s);
        $h[0]->{role} eq "system" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ai_session_history_returns_flat_array_not_wrapped_ref() {
    // Pre-fix: `my @h = ai_session_history($s)` gave len=1 (arrayref as
    // single element). Post-fix: len matches the actual row count.
    let code = r#"
        # Activate mock-embed mode via the sentinel mock — `mock_embed_active`
        # checks `match_mock("embed:probe")`, so any mock whose pattern
        # matches that literal counts.
        ai_mock_install("embed:probe", "");
        ai_mock_install('(?i)\bhi\b', "Hi back");
        my $s = ai_session_new(system => "You are helpful");
        ai_session_send($s, "hi");
        my @h = ai_session_history($s);
        scalar(@h) >= 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ai_session_history_after_reset_keeps_system() {
    let code = r#"
        # Activate mock-embed mode via the sentinel mock — `mock_embed_active`
        # checks `match_mock("embed:probe")`, so any mock whose pattern
        # matches that literal counts.
        ai_mock_install("embed:probe", "");
        ai_mock_install('(?i)\bhi\b', "Hi back");
        my $s = ai_session_new(system => "You are helpful");
        ai_session_send($s, "hi");
        ai_session_reset($s);
        my @h = ai_session_history($s);
        (scalar(@h) == 1 && $h[0]->{role} eq "system") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ai_session_history_without_system_starts_at_first_turn() {
    let code = r#"
        # Activate mock-embed mode via the sentinel mock — `mock_embed_active`
        # checks `match_mock("embed:probe")`, so any mock whose pattern
        # matches that literal counts.
        ai_mock_install("embed:probe", "");
        ai_mock_install('(?i)\bhi\b', "Hi back");
        my $s = ai_session_new();
        ai_session_send($s, "hi");
        my @h = ai_session_history($s);
        $h[0]->{role} eq "user" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ai_memory_recall: flat-array return + metadata JSON decode ───────
//
// Fix A: `strykelang/ai.rs::ai_memory_recall` — returns
// `StrykeValue::array(out)` (flat) instead of `array_ref(out)` so
// `my @hits = ai_memory_recall(...)` works without dereffing.
//
// Fix B: each row's `metadata` field is now JSON-decoded back to a
// hashref instead of returning the raw stored string. Pairs with the
// matching JSON-encode on the save side (Fix C below).
//
// All four tests use `with_global_flags` for exclusive access — the
// `ai_memory_*` store is process-global SQLite, so parallel readers
// would clobber each other's `clear`/`save`/`recall` interleavings.

#[test]
fn ai_memory_recall_returns_flat_array() {
    let code = r#"
        ai_mock_install("embed:probe", "");
        ai_memory_clear();
        ai_memory_save("d1", "stryke is fast", +{ tag => "a" });
        ai_memory_save("d2", "apples are ripe", +{ tag => "b" });
        my @r = ai_memory_recall("anything", top_k => 2);
        scalar(@r) == 2 ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

#[test]
fn ai_memory_recall_decodes_metadata_to_hashref() {
    let code = r#"
        ai_mock_install("embed:probe", "");
        ai_memory_clear();
        ai_memory_save("d1", "content", +{ category => "tech", priority => 5 });
        my @r = ai_memory_recall("content", top_k => 1);
        my ($row) = grep { $_->{id} eq "d1" } @r;
        $row->{metadata}->{category} eq "tech" ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

#[test]
fn ai_memory_recall_metadata_preserves_int() {
    let code = r#"
        ai_mock_install("embed:probe", "");
        ai_memory_clear();
        ai_memory_save("d1", "content", +{ priority => 42 });
        my @r = ai_memory_recall("content", top_k => 1);
        my ($row) = grep { $_->{id} eq "d1" } @r;
        $row->{metadata}->{priority} == 42 ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

#[test]
fn ai_memory_recall_row_has_score_and_content() {
    let code = r#"
        ai_mock_install("embed:probe", "");
        ai_memory_clear();
        ai_memory_save("d1", "hello world");
        my @r = ai_memory_recall("hello", top_k => 1);
        (exists $r[0]->{score}
            && exists $r[0]->{content}
            && exists $r[0]->{id}) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}

// ── ai_memory_save: JSON-encode hashref metadata at save time ─────────
//
// Fix C: `strykelang/ai.rs::ai_memory_save` — hashref / hashmap
// metadata is now JSON-encoded before persisting. The previous code
// called `v.to_string()` which produced `HASH(0x…)` and lost every
// key/value. Also uses `with_global_flags` for exclusive access.

#[test]
fn ai_memory_save_persists_hashref_metadata_across_recall() {
    let code = r#"
        ai_mock_install("embed:probe", "");
        ai_memory_clear();
        ai_memory_save("doc1", "Stryke is fast", +{
            category => "tech",
            tags => "rust,perl",
            priority => 1,
        });
        my @r = ai_memory_recall("Stryke", top_k => 1);
        my ($row) = grep { $_->{id} eq "doc1" } @r;
        (ref($row->{metadata}) =~ /HASH/
            && $row->{metadata}->{category} eq "tech"
            && $row->{metadata}->{priority} == 1) ? 1 : 0
    "#;
    assert_eq!(with_global_flags(|| eval_int_locked(code)), 1);
}
