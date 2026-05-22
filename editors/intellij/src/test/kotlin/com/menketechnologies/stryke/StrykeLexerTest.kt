package com.menketechnologies.stryke

import com.intellij.psi.tree.IElementType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure JUnit 4 tests for [StrykeLexer] — no IntelliJ platform fixture
 * needed. The lexer is a plain `LexerBase`; we can drive it by calling
 * `start()` + `advance()` directly and reading `tokenType` /
 * `tokenStart` / `tokenEnd`.
 *
 * These tests pin the user-facing color-preview behavior in
 * *Settings → Editor → Color Scheme → Stryke* — every category that
 * appears in [StrykeColorSettingsPage]'s DEMO must be reachable
 * through a token returned by the lexer.
 */
class StrykeLexerTest {
    /** Lex `src` end-to-end into `(type, text)` pairs. */
    private fun lex(src: String): List<Pair<IElementType, String>> {
        val lex = StrykeLexer()
        lex.start(src, 0, src.length, 0)
        val out = mutableListOf<Pair<IElementType, String>>()
        while (lex.tokenType != null) {
            out.add(lex.tokenType!! to src.substring(lex.tokenStart, lex.tokenEnd))
            lex.advance()
        }
        return out
    }

    /** True if `pairs` contains a token with the given type whose text equals `text`. */
    private fun has(pairs: List<Pair<IElementType, String>>, type: IElementType, text: String): Boolean =
        pairs.any { it.first == type && it.second == text }

    // ── in-string sigil-var interpolation (the user-reported gap) ──

    @Test
    fun double_quoted_string_with_dollar_var_emits_scalar_var_token() {
        // `"hello $name"` — the entire string used to be ONE STRING
        // token; the lexer now sub-tokenizes the `$name` as SCALAR_VAR.
        val toks = lex("\"hello \$name\"")
        assertTrue(
            "expected SCALAR_VAR `\$name` inside the string: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$name"),
        )
        // The literal prefix must come through as STRING.
        assertTrue(
            "expected STRING prefix `\"hello `: $toks",
            has(toks, StrykeTokenTypes.STRING, "\"hello "),
        )
    }

    @Test
    fun double_quoted_string_with_array_var_emits_array_var_token() {
        val toks = lex("\"items: @items end\"")
        assertTrue(
            "expected ARRAY_VAR `@items` inside the string: $toks",
            has(toks, StrykeTokenTypes.ARRAY_VAR, "@items"),
        )
    }

    @Test
    fun double_quoted_string_with_hash_subscript_emits_scalar_var() {
        // `"$h{key}"` — the `$h` part is a sigil-var; `{key}` falls
        // back to regular tokenization. At minimum, `$h` must be
        // SCALAR_VAR (the IDE colors the whole subscript as variable
        // via semantic-tokens layered on top in a real editor).
        val toks = lex("\"got \$h{key} done\"")
        assertTrue(
            "expected SCALAR_VAR `\$h` inside the string: $toks",
            toks.any { it.first == StrykeTokenTypes.SCALAR_VAR && it.second.startsWith("\$h") },
        )
    }

    @Test
    fun printf_format_specifiers_do_not_get_fake_var_highlighting() {
        // `"%-15s %8s %10s\n"` — printf format specifiers. None of
        // these should emit a HASH_VAR token; the `%` followed by a
        // digit or `-` is literal string content, NOT a hash sigil.
        val toks = lex("\"  %-15s %8s   %10s   %10s\\n\"")
        assertTrue(
            "must NOT emit HASH_VAR for printf width specs: $toks",
            toks.none { it.first == StrykeTokenTypes.HASH_VAR },
        )
        // Same for the bare `% wall` style — `%` followed by space.
        val toks2 = lex("\"% wall\"")
        assertTrue(
            "bare `% wall` must stay STRING (no HASH_VAR): $toks2",
            toks2.none { it.first == StrykeTokenTypes.HASH_VAR },
        )
    }

    @Test
    fun dollar_followed_by_digit_inside_string_does_not_break() {
        // `"got $1"` — `$1` is a regex-capture special var in Perl,
        // but per the LSP-side rule we don't highlight it as a var
        // (matches the semantic-tokens emitter behavior). The whole
        // string stays as STRING.
        val toks = lex("\"got \$1\"")
        assertTrue(
            "must NOT break string on `\$digit`: $toks",
            toks.none {
                it.first == StrykeTokenTypes.SCALAR_VAR || it.first == StrykeTokenTypes.SPECIAL_VAR
            },
        )
    }

    @Test
    fun double_quoted_string_with_no_interpolation_stays_one_string_token() {
        val toks = lex("\"plain text\"")
        // Exactly one token covering the whole string.
        assertEquals(1, toks.size)
        assertEquals(StrykeTokenTypes.STRING, toks[0].first)
        assertEquals("\"plain text\"", toks[0].second)
    }

    @Test
    fun single_quoted_string_does_not_break_on_dollar() {
        // Single quotes don't interpolate — `$var` inside `'...'` must
        // stay literal STRING text, not break into a SCALAR_VAR token.
        val toks = lex("'no \$interp here'")
        assertEquals(1, toks.size)
        assertEquals(StrykeTokenTypes.STRING, toks[0].first)
    }

    @Test
    fun dollar_sign_followed_by_punctuation_does_not_break_string() {
        // `"price: $"` — a bare `$` followed by closing quote should
        // NOT be treated as a var; it stays inside the STRING token.
        val toks = lex("\"price: \$\"")
        // The whole string should come through as a STRING token; no
        // sigil-var token emitted.
        assertTrue(
            "expected no SCALAR_VAR for a bare trailing \$: $toks",
            toks.none { it.first == StrykeTokenTypes.SCALAR_VAR },
        )
    }

    // ── basic tokenization sanity ──

    @Test
    fun line_comment_recognized() {
        val toks = lex("# hello\n")
        assertTrue("expected COMMENT: $toks", has(toks, StrykeTokenTypes.COMMENT, "# hello"))
    }

    @Test
    fun doc_comment_recognized() {
        val toks = lex("## docs\n")
        assertTrue(
            "expected DOC_COMMENT: $toks",
            toks.any { it.first == StrykeTokenTypes.DOC_COMMENT },
        )
    }

    @Test
    fun integer_and_float_distinguished() {
        val toks = lex("42 3.14")
        assertTrue("expected NUMBER 42: $toks", has(toks, StrykeTokenTypes.NUMBER, "42"))
        assertTrue("expected FLOAT 3.14: $toks", has(toks, StrykeTokenTypes.FLOAT, "3.14"))
    }

    @Test
    fun keywords_classified_by_category() {
        val toks = lex("my fn return if BEGIN true undef")
        assertTrue("DECL my: $toks", has(toks, StrykeTokenTypes.DECL_KEYWORD, "my"))
        assertTrue("FN fn: $toks", has(toks, StrykeTokenTypes.FN_KEYWORD, "fn"))
        assertTrue(
            "CONTROL return/if: $toks",
            toks.any { it.first == StrykeTokenTypes.CONTROL_KEYWORD },
        )
        assertTrue(
            "PHASE BEGIN: $toks",
            toks.any { it.first == StrykeTokenTypes.PHASE_KEYWORD },
        )
        assertTrue(
            "BOOLEAN true: $toks",
            toks.any { it.first == StrykeTokenTypes.BOOLEAN },
        )
        assertTrue(
            "UNDEF undef: $toks",
            toks.any { it.first == StrykeTokenTypes.UNDEF },
        )
    }

    @Test
    fun sigil_vars_outside_strings_classified() {
        val toks = lex("\$x @arr %h")
        assertTrue("SCALAR_VAR \$x: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$x"))
        assertTrue("ARRAY_VAR @arr: $toks", has(toks, StrykeTokenTypes.ARRAY_VAR, "@arr"))
        assertTrue("HASH_VAR %h: $toks", has(toks, StrykeTokenTypes.HASH_VAR, "%h"))
    }

    @Test
    fun arrow_fat_comma_and_pipe_classified() {
        val toks = lex("a -> b => c |> d ~> e |>> f")
        assertTrue("ARROW_OP ->: $toks", has(toks, StrykeTokenTypes.ARROW_OP, "->"))
        assertTrue("FAT_COMMA =>: $toks", has(toks, StrykeTokenTypes.FAT_COMMA, "=>"))
        assertTrue(
            "PIPE |>: $toks",
            toks.any { it.first == StrykeTokenTypes.PIPE && it.second == "|>" },
        )
        assertTrue(
            "PIPE ~>: $toks",
            toks.any { it.first == StrykeTokenTypes.PIPE && it.second == "~>" },
        )
        assertTrue(
            "PIPE |>>: $toks",
            toks.any { it.first == StrykeTokenTypes.PIPE && it.second == "|>>" },
        )
    }

    @Test
    fun string_followed_by_more_code_resumes_normal_state() {
        // Pin that after `"hello $name"` the lexer correctly returns
        // to STATE_NORMAL — a following `my $x` must tokenize cleanly.
        val toks = lex("\"hi \$name\"\nmy \$x = 1\n")
        assertTrue("STRING prefix: $toks", has(toks, StrykeTokenTypes.STRING, "\"hi "))
        assertTrue("SCALAR_VAR in-string: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$name"))
        assertTrue("DECL my: $toks", has(toks, StrykeTokenTypes.DECL_KEYWORD, "my"))
        assertTrue("SCALAR_VAR \$x: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$x"))
        assertTrue(
            "NUMBER 1: $toks",
            toks.any { it.first == StrykeTokenTypes.NUMBER && it.second == "1" },
        )
    }
}
