package com.menketechnologies.stryke

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Smoke tests for [StrykeColorSettingsPage] — pin that every category
 * the descriptor list exposes is exercised by at least one token in
 * the demo text. Without these, the demo can rot silently (an
 * AttributesDescriptor without a matching token in DEMO renders as a
 * dead entry the user can rebind but never see).
 */
class StrykeColorSettingsPageTest {
    private val page = StrykeColorSettingsPage()

    @Test
    fun demo_text_is_non_empty() {
        val demo = page.getDemoText()
        assertTrue("DEMO should be non-empty", demo.isNotBlank())
        // At least ~30 lines so every category gets a visible row.
        assertTrue(
            "DEMO should have ≥20 lines (currently ${demo.lines().size})",
            demo.lines().size >= 20,
        )
    }

    @Test
    fun demo_contains_each_user_visible_category_marker() {
        val demo = page.getDemoText()
        // Markers chosen to be uniquely identifying for a category.
        // If any of these strings disappears from DEMO, the corresponding
        // color row in the Settings page no longer has a visible example.
        val required = listOf(
            "# " to "Comments//Line comment",
            "## " to "Comments//Doc comment",
            "\"" to "Strings//String",
            "\\t" to "Strings//String escape",
            "<<~" to "Strings//Heredoc",
            "`ls" to "Backtick qx",
            "1_000_000" to "Numbers//Integer",
            "3.14" to "Numbers//Float",
            "/^(" to "Regex//Pattern",
            "/i" to "Regex//Flags",
            "my " to "Keywords//Declaration",
            "fn " to "Keywords//Function/class",
            "if " to "Keywords//Control flow",
            "BEGIN" to "Keywords//Phase",
            " and " to "Keywords//Word operator (and)",
            " or " to "Keywords//Word operator (or)",
            " eq " to "Keywords//Word operator (eq)",
            " cmp " to "Keywords//Word operator (cmp)",
            "true" to "Keywords//Boolean true",
            "false" to "Keywords//Boolean false",
            "undef" to "Keywords//undef",
            "Demo::Anagram" to "Names//Package name",
            "::" to "Names//Package separator",
            "OUTER:" to "Names//Label (loop)",
            "last OUTER" to "Names//Label ref",
            "\$word" to "Variables//Scalar variable",
            "@chars" to "Variables//Array variable",
            "%h" to "Variables//Hash variable",
            "\$!" to "Variables//Special variable",
            "\$_" to "Variables//Topic",
            "_0" to "Variables//Block parameter",
            "->" to "Operators//Arrow",
            "=>" to "Operators//Fat comma",
            "|>" to "Operators//Pipe",
            "=~" to "Operators//Regex bind",
            "1..3" to "Operators//Range (..)",
            "1:1_000_000" to "Operators//Range (:)",
            "split" to "Names//Builtin",
            "pmap" to "Names//Parallel builtin",
            // In-string interpolation — the case the user originally reported.
            "\"hi \$name" to "In-string \$var interpolation",
        )
        val missing = required.filter { (needle, _) -> !demo.contains(needle) }
        assertTrue(
            "DEMO missing category markers:\n${missing.joinToString("\n") { "  ${it.first}  (${it.second})" }}\n\nDEMO=\n$demo",
            missing.isEmpty(),
        )
    }

    @Test
    fun attribute_descriptors_present_and_unique() {
        val descs = page.attributeDescriptors
        assertTrue("expected ≥30 categories", descs.size >= 30)
        // Each AttributesDescriptor displayName must be unique — duplicates
        // confuse the user (two rows that look the same but bind different
        // TextAttributesKeys).
        val names = descs.map { it.displayName }
        assertEquals(
            "duplicate displayName entries: ${
                names.groupingBy { it }.eachCount().filter { it.value > 1 }
            }",
            names.toSet().size,
            names.size,
        )
    }

    @Test
    fun display_name_is_stryke() {
        assertEquals("Stryke", page.displayName)
    }
}
