package com.menketechnologies.stryke

import com.menketechnologies.stryke.StrykeSmartEnterProcessor.Companion.Plan
import com.menketechnologies.stryke.StrykeSmartEnterProcessor.Companion.computePlan
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Pure JUnit 4 tests for the smart-enter planner — drives
 * [StrykeSmartEnterProcessor.computePlan] directly so no IntelliJ
 * platform fixture is needed.
 *
 * Convention in each test:
 *
 *  * The `src` parameter is the document text; `|` marks the user's
 *    caret position (stripped before passing to `computePlan`).
 *  * `expected` is the text after applying the plan, again with `|`
 *    for where the caret should land.
 */
class StrykeSmartEnterProcessorTest {
    private fun caretOf(src: String): Pair<String, Int> {
        val i = src.indexOf('|')
        require(i >= 0) { "test fixture must contain '|' for caret: $src" }
        return src.removeRange(i, i + 1) to i
    }

    private fun applyPlan(src: String): String {
        val (text, caret) = caretOf(src)
        val lineStart = text.lastIndexOf('\n', caret - 1).let { if (it < 0) 0 else it + 1 }
        val lineEnd = text.indexOf('\n', caret).let { if (it < 0) text.length else it }
        val line = text.substring(lineStart, lineEnd)
        val plan = computePlan(line, lineStart, caret, text)
            ?: error("expected a plan for: $src")
        val sb = StringBuilder(text)
        sb.insert(plan.offset, plan.insert)
        sb.insert(plan.offset + plan.caretRel, "|")
        return sb.toString()
    }

    private fun assertPlan(src: String, expected: String) {
        assertEquals(expected, applyPlan(src))
    }

    private fun assertNoPlan(src: String) {
        val (text, caret) = caretOf(src)
        val lineStart = text.lastIndexOf('\n', caret - 1).let { if (it < 0) 0 else it + 1 }
        val lineEnd = text.indexOf('\n', caret).let { if (it < 0) text.length else it }
        val line = text.substring(lineStart, lineEnd)
        assertNull(
            "expected no plan for: $src",
            computePlan(line, lineStart, caret, text),
        )
    }

    // ── Strategy 1: paren-header keyword + body ────────────────────

    @Test fun fn_with_closing_paren_adds_body() {
        assertPlan(
            "fn Backend::call(\$pay|)",
            "fn Backend::call(\$pay) {\n    |\n}",
        )
    }

    @Test fun fn_without_closing_paren_balances_then_adds_body() {
        assertPlan(
            "fn Backend::call(\$pay|",
            "fn Backend::call(\$pay) {\n    |\n}",
        )
    }

    @Test fun fn_already_has_body_is_noop() {
        // `{` already follows the signature — strategy must bail.
        assertNoPlan("fn foo()| {\n}")
    }

    @Test fun if_paren_complete_then_body() {
        assertPlan(
            "if (\$x > 0|)",
            "if (\$x > 0) {\n    |\n}",
        )
    }

    @Test fun while_with_unclosed_paren() {
        assertPlan(
            "while (len @xs > 0|",
            "while (len @xs > 0) {\n    |\n}",
        )
    }

    @Test fun foreach_my_var_in_list() {
        assertPlan(
            "foreach my \$x (@xs|)",
            "foreach my \$x (@xs) {\n    |\n}",
        )
    }

    @Test fun elsif_with_condition() {
        assertPlan(
            "elsif (\$y < 0|)",
            "elsif (\$y < 0) {\n    |\n}",
        )
    }

    @Test fun method_decl_with_params() {
        assertPlan(
            "method greet(\$name|)",
            "method greet(\$name) {\n    |\n}",
        )
    }

    @Test fun fn_preserves_existing_indent() {
        assertPlan(
            "    fn inner(\$x|)",
            "    fn inner(\$x) {\n        |\n    }",
        )
    }

    // ── Strategy 2: type-decl / bare-block keyword ─────────────────

    @Test fun class_decl_adds_body() {
        assertPlan(
            "class Backend|",
            "class Backend {\n    |\n}",
        )
    }

    @Test fun class_extends_clause_then_body() {
        assertPlan(
            "class Animal extends Mammal|",
            "class Animal extends Mammal {\n    |\n}",
        )
    }

    @Test fun struct_decl_adds_body() {
        assertPlan(
            "struct Point|",
            "struct Point {\n    |\n}",
        )
    }

    @Test fun trait_decl_adds_body() {
        assertPlan(
            "trait Drawable|",
            "trait Drawable {\n    |\n}",
        )
    }

    @Test fun enum_decl_adds_body() {
        assertPlan(
            "enum Color|",
            "enum Color {\n    |\n}",
        )
    }

    @Test fun impl_decl_adds_body() {
        assertPlan(
            "impl Display for Point|",
            "impl Display for Point {\n    |\n}",
        )
    }

    @Test fun class_with_incomplete_extends_clause_is_noop() {
        // User is mid-typing the parent name — don't slam the body on.
        assertNoPlan("class Foo extends|")
    }

    @Test fun class_with_incomplete_is_clause_is_noop() {
        assertNoPlan("class Foo is|")
    }

    @Test fun bare_else_adds_body() {
        assertPlan(
            "else|",
            "else {\n    |\n}",
        )
    }

    @Test fun bare_do_adds_body() {
        assertPlan(
            "do|",
            "do {\n    |\n}",
        )
    }

    @Test fun bare_try_adds_body() {
        assertPlan(
            "try|",
            "try {\n    |\n}",
        )
    }

    @Test fun bare_catch_adds_body() {
        assertPlan(
            "catch|",
            "catch {\n    |\n}",
        )
    }

    @Test fun bare_begin_adds_body() {
        assertPlan(
            "BEGIN|",
            "BEGIN {\n    |\n}",
        )
    }

    @Test fun decl_with_existing_body_is_noop() {
        assertNoPlan("class Foo| {\n}")
    }

    @Test fun decl_with_semicolon_terminator_is_noop() {
        // `class Foo;` — Perl-style fwd-decl; user explicitly said no body.
        assertNoPlan("class Foo;|")
    }

    // ── Strategy 3: bracket balance on the current line ────────────

    @Test fun unclosed_paren_in_call_balances_at_eol() {
        assertPlan(
            "p len(@xs|",
            "p len(@xs)|",
        )
    }

    @Test fun unclosed_bracket_balances_at_eol() {
        assertPlan(
            "my @arr = [1, 2, 3|",
            "my @arr = [1, 2, 3]|",
        )
    }

    @Test fun unclosed_nested_balances_in_correct_order() {
        // Outer `(`, inner `[` — must close as `])`.
        assertPlan(
            "p map { _ * 2 } ([1, 2|",
            "p map { _ * 2 } ([1, 2])|",
        )
    }

    @Test fun balanced_line_is_noop() {
        // Already balanced — no structural completion to do.
        assertNoPlan("p len(@xs)|")
    }

    @Test fun comment_line_is_noop() {
        assertNoPlan("# fn Foo(\$x|")
    }

    @Test fun unclosed_paren_inside_string_does_not_count() {
        // The `(` inside `"..."` is literal text, not a missing paren.
        assertNoPlan("p \"hello (world|\"")
    }

    @Test fun comment_after_unclosed_paren_does_not_count_inside_comment() {
        // The `]` in the comment is irrelevant; `(` is still unclosed.
        assertPlan(
            "p len(@xs|  # see ] note",
            "p len(@xs  # see ] note)|",
        )
    }
}
