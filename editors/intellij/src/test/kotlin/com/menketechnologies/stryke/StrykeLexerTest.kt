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
        // `"$h{key}"` — `$h{key}` must come through as one SCALAR_VAR
        // token. Before the extendStringInterpSubscripts fix the lexer
        // dropped back to STRING mode at `{`, leaving the subscript
        // uncolored — the bug the user hit on
        // `"counts: red=$counts{red}, blue=$counts{blue}"`.
        val toks = lex("\"got \$h{key} done\"")
        assertTrue(
            "expected SCALAR_VAR `\$h{key}` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$h{key}"),
        )
    }

    @Test
    fun double_quoted_string_with_array_subscript_emits_scalar_var() {
        // `"$ary[0]"` — same gap as the hash-subscript case, on the
        // `[N]` path.
        val toks = lex("\"v=\$ary[0] done\"")
        assertTrue(
            "expected SCALAR_VAR `\$ary[0]` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$ary[0]"),
        )
    }

    @Test
    fun double_quoted_string_with_chained_hash_subscripts_emits_scalar_var() {
        // `"$h{k1}{k2}"` — chained subscripts. Both braces must be
        // absorbed into the SCALAR_VAR token, not just the first.
        val toks = lex("\"x=\$h{k1}{k2} done\"")
        assertTrue(
            "expected SCALAR_VAR `\$h{k1}{k2}` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$h{k1}{k2}"),
        )
    }

    @Test
    fun double_quoted_string_with_mixed_subscripts_emits_scalar_var() {
        // `"$h{k}[0]{m}"` — mixed `{}`/`[]` chain.
        val toks = lex("\"x=\$h{k}[0]{m} done\"")
        assertTrue(
            "expected SCALAR_VAR `\$h{k}[0]{m}` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$h{k}[0]{m}"),
        )
    }

    @Test
    fun double_quoted_string_with_arrow_hash_deref_emits_scalar_var() {
        // `"$ref->{key}"` — Perl-style arrow into hash subscript.
        val toks = lex("\"v=\$ref->{key} done\"")
        assertTrue(
            "expected SCALAR_VAR `\$ref->{key}` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$ref->{key}"),
        )
    }

    @Test
    fun double_quoted_string_with_arrow_array_deref_emits_scalar_var() {
        // `"$ref->[0]"` — Perl-style arrow into array subscript.
        val toks = lex("\"v=\$ref->[0] done\"")
        assertTrue(
            "expected SCALAR_VAR `\$ref->[0]` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$ref->[0]"),
        )
    }

    @Test
    fun double_quoted_string_with_chained_arrow_deref_emits_scalar_var() {
        // `"$ref->{k}->[0]->{m}"` — chained arrow derefs over mixed
        // subscript kinds. The whole chain is one variable token.
        val toks = lex("\"v=\$ref->{k}->[0]->{m} done\"")
        assertTrue(
            "expected SCALAR_VAR `\$ref->{k}->[0]->{m}` as one token: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$ref->{k}->[0]->{m}"),
        )
    }

    @Test
    fun double_quoted_string_arrow_method_call_does_not_extend() {
        // Perl does NOT interpolate method calls — `"$obj->method"`
        // emits `$obj` and leaves the rest as STRING. The extender
        // must stop at `->` when the next char is not `{` or `[`.
        val toks = lex("\"name=\$obj->method done\"")
        assertTrue(
            "expected SCALAR_VAR `\$obj` (NOT extended through `->method`): $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$obj"),
        )
        assertTrue(
            "expected the `->method done\"` tail to come through as STRING: $toks",
            toks.any { it.first == StrykeTokenTypes.STRING && it.second.contains("->method") },
        )
    }

    @Test
    fun printf_format_specifiers_get_string_format_tokens() {
        // `"%-15s %8d %10.2f\n"` — each `%...conv` chunk is one
        // STRING_FORMAT token, distinct from the surrounding STRING.
        val toks = lex("\"%-15s %8d %10.2f\\n\"")
        val fmtTokens = toks.filter { it.first == StrykeTokenTypes.STRING_FORMAT }
        assertTrue(
            "expected ≥3 STRING_FORMAT tokens: $toks",
            fmtTokens.size >= 3,
        )
        assertTrue(
            "expected `%-15s` as STRING_FORMAT: $toks",
            fmtTokens.any { it.second == "%-15s" },
        )
        assertTrue(
            "expected `%8d` as STRING_FORMAT: $toks",
            fmtTokens.any { it.second == "%8d" },
        )
        assertTrue(
            "expected `%10.2f` as STRING_FORMAT: $toks",
            fmtTokens.any { it.second == "%10.2f" },
        )
        // No fake HASH_VAR.
        assertTrue(
            "must NOT emit HASH_VAR for printf format: $toks",
            toks.none { it.first == StrykeTokenTypes.HASH_VAR },
        )
    }

    @Test
    fun bare_percent_string_does_not_break_highlight() {
        // `"%"` (modulo operator as a string key, e.g. `%PREC = ("%" => 3)`)
        // — `%` followed by `"` is NOT a format spec; the whole string
        // stays as ONE STRING token, no spurious break.
        val toks = lex("\"%\"")
        assertEquals(
            "expected single STRING token for `\"%\"`: $toks",
            1, toks.size,
        )
        assertEquals(StrykeTokenTypes.STRING, toks[0].first)
        assertEquals("\"%\"", toks[0].second)
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
        val toks = lex("my var val fn return if BEGIN true undef")
        assertTrue("DECL my: $toks", has(toks, StrykeTokenTypes.DECL_KEYWORD, "my"))
        assertTrue("DECL var: $toks", has(toks, StrykeTokenTypes.DECL_KEYWORD, "var"))
        assertTrue("DECL val: $toks", has(toks, StrykeTokenTypes.DECL_KEYWORD, "val"))
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
    fun keyword_sets_match_authoritative_list() {
        // These keywords were previously missing from the lexer's keyword sets
        // and fell through to BUILTIN / IDENTIFIER coloring. They are all in the
        // language's authoritative `%k` keyword table.
        val decls = lex("import const frozen typed oursync")
        for (kw in listOf("import", "const", "frozen", "typed", "oursync")) {
            assertTrue(
                "$kw must be DECL_KEYWORD: $decls",
                has(decls, StrykeTokenTypes.DECL_KEYWORD, kw),
            )
        }
        val ctrl = lex("continue goto defer match")
        for (kw in listOf("continue", "goto", "defer", "match")) {
            assertTrue(
                "$kw must be CONTROL_KEYWORD: $ctrl",
                has(ctrl, StrykeTokenTypes.CONTROL_KEYWORD, kw),
            )
        }
        // `match` is a keyword, not a builtin.
        assertTrue(
            "match must NOT be BUILTIN: $ctrl",
            ctrl.none { it.first == StrykeTokenTypes.BUILTIN && it.second == "match" },
        )
        val conc = lex("mysync varsync")
        assertTrue("mysync parallel: $conc", has(conc, StrykeTokenTypes.BUILTIN_PARALLEL, "mysync"))
        assertTrue("varsync parallel: $conc", has(conc, StrykeTokenTypes.BUILTIN_PARALLEL, "varsync"))
        val magic = lex("__FILE__ __LINE__ __PACKAGE__")
        for (mc in listOf("__FILE__", "__LINE__", "__PACKAGE__")) {
            assertTrue(
                "$mc must be KEYWORD: $magic",
                has(magic, StrykeTokenTypes.KEYWORD, mc),
            )
        }
    }

    @Test
    fun sigil_vars_outside_strings_classified() {
        val toks = lex("\$x @arr %h")
        assertTrue("SCALAR_VAR \$x: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$x"))
        assertTrue("ARRAY_VAR @arr: $toks", has(toks, StrykeTokenTypes.ARRAY_VAR, "@arr"))
        assertTrue("HASH_VAR %h: $toks", has(toks, StrykeTokenTypes.HASH_VAR, "%h"))
    }

    @Test
    fun fn_declaration_name_is_function_decl_token() {
        // `fn foo` — `foo` is a FUNCTION_DECL, not a plain IDENTIFIER.
        val toks = lex("fn foo { 1 }")
        assertTrue(
            "expected FUNCTION_DECL `foo`: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_DECL, "foo"),
        )
    }

    @Test
    fun call_site_name_is_function_call_token() {
        // `foo()` at call site — `foo` is a FUNCTION_CALL.
        val toks = lex("foo()")
        assertTrue(
            "expected FUNCTION_CALL `foo`: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_CALL, "foo"),
        )
    }

    @Test
    fun loop_label_is_label_token() {
        // `OUTER:` — `OUTER` followed by single `:` is a LABEL.
        val toks = lex("OUTER: for my \$i (1..3) { last OUTER }")
        assertTrue(
            "expected LABEL `OUTER`: $toks",
            has(toks, StrykeTokenTypes.LABEL, "OUTER"),
        )
    }

    @Test
    fun package_separator_is_distinct_from_package_name() {
        // `Foo::Bar` — `Foo` PACKAGE_NAME, `::` PACKAGE_SEPARATOR,
        // `Bar` PACKAGE_NAME. The separator must be its own token so
        // the user can color it independently in Settings.
        val toks = lex("my \$x = Foo::Bar")
        assertTrue(
            "expected PACKAGE_NAME `Foo`: $toks",
            has(toks, StrykeTokenTypes.PACKAGE_NAME, "Foo"),
        )
        assertTrue(
            "expected PACKAGE_SEPARATOR `::`: $toks",
            has(toks, StrykeTokenTypes.PACKAGE_SEPARATOR, "::"),
        )
        assertTrue(
            "expected PACKAGE_NAME / IDENTIFIER for `Bar`: $toks",
            toks.any {
                it.second == "Bar"
                    && (it.first == StrykeTokenTypes.PACKAGE_NAME
                        || it.first == StrykeTokenTypes.IDENTIFIER)
            },
        )
    }

    @Test
    fun regex_flags_emitted_as_distinct_token() {
        // `/abc/igs` — `/abc/` is REGEX, `igs` is REGEX_FLAGS.
        val toks = lex("/abc/igs")
        assertTrue(
            "expected REGEX `/abc/`: $toks",
            has(toks, StrykeTokenTypes.REGEX, "/abc/"),
        )
        assertTrue(
            "expected REGEX_FLAGS `igs`: $toks",
            has(toks, StrykeTokenTypes.REGEX_FLAGS, "igs"),
        )
    }

    @Test
    fun block_param_outer_chain_chevrons_are_one_token() {
        // `_<<<<<` — 5-deep outer chain bare form.
        val toks = lex("_<<<<<")
        assertTrue(
            "expected `_<<<<<` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_<<<<<"),
        )
    }

    @Test
    fun block_param_indexed_ascent_is_one_token() {
        // `_<5` — indexed-ascent shortcut.
        val toks = lex("_<5")
        assertTrue(
            "expected `_<5` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_<5"),
        )
    }

    @Test
    fun block_param_positional_outer_chain_combined() {
        // `_2<<<` — positional + outer chain.
        val toks = lex("_2<<<")
        assertTrue(
            "expected `_2<<<` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_2<<<"),
        )
    }

    @Test
    fun block_param_sigiled_outer_chain() {
        // `\$_<<<<<` — sigil-prefixed outer chain.
        val toks = lex("\$_<<<<<")
        assertTrue(
            "expected `\$_<<<<<` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "\$_<<<<<"),
        )
    }

    @Test
    fun block_param_sigiled_indexed_ascent() {
        val toks = lex("\$_<3")
        assertTrue(
            "expected `\$_<3` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "\$_<3"),
        )
    }

    @Test
    fun block_param_single_chevron_outer() {
        // `_<` — single-chevron outer-topic shortcut (one level up).
        // Must lex as ONE BLOCK_PARAM token, not `_` + `<`.
        val toks = lex("_<")
        assertTrue(
            "expected `_<` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_<"),
        )
    }

    @Test
    fun block_param_single_chevron_with_digit() {
        // `_<2` — indexed-ascent shortcut, two levels up.
        val toks = lex("_<2")
        assertTrue(
            "expected `_<2` as one BLOCK_PARAM token: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_<2"),
        )
    }

    @Test
    fun keyword_inside_hash_subscript_becomes_bareword() {
        // `$tl->{state}` — `state` is a hash key, NOT the `state` decl
        // keyword. Must classify as IDENTIFIER so the IDE doesn't paint
        // it as a keyword.
        val toks = lex("\$tl->{state}")
        assertTrue(
            "`state` inside `->{...}` must be IDENTIFIER not DECL_KEYWORD: $toks",
            has(toks, StrykeTokenTypes.IDENTIFIER, "state"),
        )
        assertTrue(
            "must NOT classify `state` as DECL_KEYWORD here: $toks",
            toks.none { it.first == StrykeTokenTypes.DECL_KEYWORD && it.second == "state" },
        )
    }

    @Test
    fun keyword_inside_sigil_hash_subscript_becomes_bareword() {
        // `$h{state}` — same rule, no `->` needed.
        val toks = lex("\$h{state}")
        assertTrue(
            "`state` inside `\$h{...}` must be IDENTIFIER: $toks",
            has(toks, StrykeTokenTypes.IDENTIFIER, "state"),
        )
        assertTrue(
            "must NOT classify `state` as DECL_KEYWORD here: $toks",
            toks.none { it.first == StrykeTokenTypes.DECL_KEYWORD && it.second == "state" },
        )
    }

    @Test
    fun keyword_before_fat_comma_becomes_bareword() {
        // `(state => 1)` — fat-comma autoquotes `state`, must be IDENTIFIER.
        val toks = lex("(state => 1)")
        assertTrue(
            "`state` before `=>` must be IDENTIFIER: $toks",
            has(toks, StrykeTokenTypes.IDENTIFIER, "state"),
        )
        assertTrue(
            "must NOT classify `state` as DECL_KEYWORD here: $toks",
            toks.none { it.first == StrykeTokenTypes.DECL_KEYWORD && it.second == "state" },
        )
    }

    @Test
    fun real_state_keyword_outside_hash_still_classified() {
        // Top-level `state $x = 1` — `state` IS the decl keyword here.
        val toks = lex("state \$x = 1")
        assertTrue(
            "top-level `state` must remain DECL_KEYWORD: $toks",
            has(toks, StrykeTokenTypes.DECL_KEYWORD, "state"),
        )
    }

    @Test
    fun multiple_keyword_hash_keys_all_become_barewords() {
        // `$tl->{state} + $tl->{my} + $tl->{for}` — every keyword that
        // happens to be a hash key must classify as IDENTIFIER.
        val toks = lex("\$tl->{state} + \$tl->{my} + \$tl->{for}")
        for (key in listOf("state", "my", "for")) {
            assertTrue(
                "`$key` as hash key must be IDENTIFIER: $toks",
                toks.any { it.first == StrykeTokenTypes.IDENTIFIER && it.second == key },
            )
        }
    }

    @Test
    fun keyword_spelling_after_fn_is_function_decl_not_keyword() {
        // `trait Stateful { fn state; fn transition }` — `state` is a
        // method name here, not the `state` decl keyword. After `fn`
        // (FN_KEYWORD intro), the next word is always a FUNCTION_DECL
        // regardless of its keyword classification.
        val toks = lex("fn state { 1 }")
        assertTrue(
            "`state` after `fn` must be FUNCTION_DECL, not DECL_KEYWORD: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_DECL, "state"),
        )
        assertTrue(
            "must NOT classify `state` as DECL_KEYWORD here: $toks",
            toks.none { it.first == StrykeTokenTypes.DECL_KEYWORD && it.second == "state" },
        )
    }

    @Test
    fun keyword_spelling_after_sub_is_function_decl() {
        val toks = lex("sub for {}")
        assertTrue(
            "`for` after `sub` must be FUNCTION_DECL: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_DECL, "for"),
        )
        assertTrue(
            "must NOT classify `for` as CONTROL_KEYWORD here: $toks",
            toks.none { it.first == StrykeTokenTypes.CONTROL_KEYWORD && it.second == "for" },
        )
    }

    @Test
    fun trait_method_with_keyword_spelling_classified_as_decl() {
        // Full trait body — both methods named after keywords.
        val toks = lex("trait Stateful {\n    fn state\n    fn transition\n}")
        assertTrue(
            "trait method `state` must be FUNCTION_DECL: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_DECL, "state"),
        )
        assertTrue(
            "trait method `transition` must be FUNCTION_DECL: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_DECL, "transition"),
        )
    }

    @Test
    fun substitution_with_embedded_double_quote_lexes_as_one_regex() {
        // `$q =~ s/"/""/g` — the embedded `"` chars are part of the
        // substitution, NOT string-literal delimiters. The whole
        // `s/"/""/g` must lex as ONE REGEX token so the lexer doesn't
        // think there's an unbalanced `"` and render the rest of the
        // file as STRING content.
        val src = "\$q =~ s/\"/\"\"/g\np \"done\""
        val toks = lex(src)
        assertTrue(
            "expected `s/\"/\"\"/g` as one REGEX token: $toks",
            has(toks, StrykeTokenTypes.REGEX, "s/\"/\"\"/g"),
        )
        // The `"done"` AFTER the substitution must still tokenize as
        // a normal STRING (the lexer state is back to NORMAL).
        assertTrue(
            "expected `\"done\"` as STRING after substitution: $toks",
            toks.any { it.first == StrykeTokenTypes.STRING && it.second.contains("done") },
        )
    }

    @Test
    fun substitution_two_segment_forms() {
        for (src in listOf(
            "\$x =~ s/foo/bar/g",
            "\$x =~ tr/a-z/A-Z/",
            "\$x =~ y/aeiou//d",
            "\$x =~ s{abc}{xyz}gi",
            "\$x =~ s|http|https|g",
        )) {
            val toks = lex(src)
            assertTrue(
                "$src — expected exactly one REGEX token covering the op: $toks",
                toks.count { it.first == StrykeTokenTypes.REGEX } == 1,
            )
        }
    }

    @Test
    fun match_single_segment_forms() {
        for ((src, expected) in listOf(
            "\$x =~ m/foo/i" to "m/foo/i",
            "\$x =~ qr/bar/" to "qr/bar/",
            "\$x =~ m{baz}" to "m{baz}",
        )) {
            val toks = lex(src)
            assertTrue(
                "$src — expected REGEX token `$expected`: $toks",
                has(toks, StrykeTokenTypes.REGEX, expected),
            )
        }
    }

    @Test
    fun perl_style_array_ref_interpolation_lexes_interior_as_code() {
        // `"foo @{[ bar() ]} baz"` — `@{[ EXPR ]}` is Perl-style array
        // interpolation, common in heredocs and double-quoted strings.
        // The IDE must color the interior as code, not as literal text.
        val toks = lex("\"foo @{[ bar() ]} baz\"")
        // The literal prefix `"foo ` is one STRING token.
        assertTrue(
            "expected STRING prefix `\"foo `: $toks",
            has(toks, StrykeTokenTypes.STRING, "\"foo "),
        )
        // The `@{[` opener is its own OPERATOR token.
        assertTrue(
            "expected OPERATOR `@{[`: $toks",
            has(toks, StrykeTokenTypes.OPERATOR, "@{["),
        )
        // Interior `bar` is a FUNCTION_CALL (followed by `(`), proving
        // we're tokenizing as code, not as string text.
        assertTrue(
            "expected FUNCTION_CALL `bar` inside `@{[ ... ]}`: $toks",
            has(toks, StrykeTokenTypes.FUNCTION_CALL, "bar"),
        )
        // Closing `]}` is one OPERATOR token.
        assertTrue(
            "expected OPERATOR `]}`: $toks",
            has(toks, StrykeTokenTypes.OPERATOR, "]}"),
        )
        // The suffix ` baz"` resumes as STRING.
        assertTrue(
            "expected STRING suffix ` baz\"`: $toks",
            has(toks, StrykeTokenTypes.STRING, " baz\""),
        )
        // The bare `@` of `@{[` must NOT have been tokenized as an
        // ARRAY_VAR — that would let the user's eye misread it as
        // `@var`.
        assertTrue(
            "no spurious ARRAY_VAR from `@{[`: $toks",
            toks.none {
                it.first == StrykeTokenTypes.ARRAY_VAR && it.second.startsWith("@{")
            },
        )
    }

    @Test
    fun nested_thread_map_block_params_all_recognized() {
        // Real-world fixture from user: triple-nested `~>` map with
        // `_`, `_<`, `_<2` in the innermost block. Every block-param
        // form must be its own BLOCK_PARAM token; the `+` operators
        // between them stay as OPERATORs.
        val src = "~>> (1:1) map { _ + _< + _<2 }"
        val toks = lex(src)
        assertTrue(
            "expected `_` as BLOCK_PARAM: $toks",
            has(toks, StrykeTokenTypes.TOPIC_VAR, "_") ||
                has(toks, StrykeTokenTypes.BLOCK_PARAM, "_"),
        )
        assertTrue(
            "expected `_<` as BLOCK_PARAM: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_<"),
        )
        assertTrue(
            "expected `_<2` as BLOCK_PARAM: $toks",
            has(toks, StrykeTokenTypes.BLOCK_PARAM, "_<2"),
        )
    }

    @Test
    fun thread_arrow_variants_all_classified_as_pipe() {
        // All thread-arrow forms must be single PIPE tokens.
        for ((src, label) in listOf(
            "~>" to "thread-first",
            "~>>" to "thread-last",
            "~s>" to "stream-first",
            "~s>>" to "stream-last",
            "~p>" to "parallel-first",
            "~p>>" to "parallel-last",
            "~d>" to "dist-first",
            "~d>>" to "dist-last",
        )) {
            val toks = lex(src)
            assertTrue(
                "expected `$src` ($label) as one PIPE token: $toks",
                has(toks, StrykeTokenTypes.PIPE, src),
            )
        }
    }

    @Test
    fun arrow_fat_comma_and_pipe_classified() {
        // `|>>` is NOT a 3-char operator — the Rust lexer tokenizes
        // it as `|>` followed by `>`. Test only the real forms.
        val toks = lex("a -> b => c |> d ~> e ~>> f")
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
            "PIPE ~>>: $toks",
            toks.any { it.first == StrykeTokenTypes.PIPE && it.second == "~>>" },
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

    // ── `$#var` is not a comment ──
    //
    // Regression pin (2026-05-23 user-reported): the line
    //   `@xs ? [$xs[0] + 1, @xs[1..$#xs]] : [1]`
    // had everything after `$#xs` painted as COMMENT. `#` is a
    // comment opener UNLESS preceded by `$` (then it's `$#var` =
    // last-index-of) or `${` (then it's `${#var}` = string length).

    @Test
    fun dollar_hash_var_is_scalar_var_not_comment() {
        val toks = lex("\$#xs")
        assertTrue("expected SCALAR_VAR `\$#xs`: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$#xs"))
        assertTrue("no COMMENT should appear: $toks", toks.none { it.first == StrykeTokenTypes.COMMENT })
    }

    @Test
    fun dollar_hash_bare_is_scalar_var_not_comment() {
        val toks = lex("\$#")
        assertTrue("expected SCALAR_VAR `\$#`: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$#"))
    }

    @Test
    fun dollar_hash_var_in_expression_does_not_eat_rest_of_line() {
        // The exact source from the user's screenshot.
        val toks = lex("@xs ? [\$xs[0] + 1, @xs[1..\$#xs]] : [1]")
        assertTrue("SCALAR_VAR \$#xs: $toks", has(toks, StrykeTokenTypes.SCALAR_VAR, "\$#xs"))
        // The closing `]` `]` and ternary `:` must still be regular
        // tokens, NOT swallowed by a comment.
        assertTrue("no comment token: $toks", toks.none { it.first == StrykeTokenTypes.COMMENT })
        // The `[1]` ternary-else branch must still tokenize.
        assertTrue(
            "trailing NUMBER 1: $toks",
            toks.any { it.first == StrykeTokenTypes.NUMBER && it.second == "1" },
        )
    }

    @Test
    fun real_comment_still_works() {
        // Sanity: a real comment (`#` after whitespace) is still a comment.
        val toks = lex("foo  # actual comment\nbar")
        assertTrue(
            "expected COMMENT `# actual comment`: $toks",
            toks.any { it.first == StrykeTokenTypes.COMMENT && it.second.startsWith("# actual") },
        )
    }

    // ── caret-style special vars (`%^HOOK`, `%main::^HOOK`) ──

    @Test
    fun bare_sigil_caret_special_var_keeps_tail() {
        // `%^HOOK` was previously split into `%^` (SPECIAL_VAR) + `HOOK`
        // (IDENTIFIER). The lexer must consume the entire caret-tail
        // identifier as part of the SPECIAL_VAR token.
        val toks = lex("p %^HOOK")
        assertTrue("expected SPECIAL_VAR `%^HOOK`: $toks", has(toks, StrykeTokenTypes.SPECIAL_VAR, "%^HOOK"))
        assertTrue("no stray IDENTIFIER `HOOK`: $toks", toks.none { it.second == "HOOK" })
    }

    @Test
    fun pkg_qualified_caret_special_var_is_one_token() {
        // `%main::^HOOK` was tokenized as `%main::` + `^` + `HOOK`. The
        // whole sequence should be one HASH_VAR token instead.
        val toks = lex("p %main::^HOOK")
        assertTrue(
            "expected HASH_VAR `%main::^HOOK`: $toks",
            has(toks, StrykeTokenTypes.HASH_VAR, "%main::^HOOK"),
        )
        assertTrue(
            "no stray OPERATOR `^`: $toks",
            toks.none { it.first == StrykeTokenTypes.OPERATOR && it.second == "^" },
        )
    }

    @Test
    fun scalar_pkg_qualified_caret_special_var_is_one_token() {
        val toks = lex("p \$main::^W")
        assertTrue(
            "expected SCALAR_VAR `\$main::^W`: $toks",
            has(toks, StrykeTokenTypes.SCALAR_VAR, "\$main::^W"),
        )
    }

    // ── code-context block-deref `@{ EXPR }` (the user-reported bug) ──

    @Test
    fun simple_array_block_deref_is_one_array_var_token() {
        // `@{name}` and `@{ $ref }` wrap a bare symbolic name — still
        // lumped into a single ARRAY_VAR token.
        val toks = lex("@{name}")
        assertTrue(
            "expected ARRAY_VAR `@{name}`: $toks",
            has(toks, StrykeTokenTypes.ARRAY_VAR, "@{name}"),
        )
        val spaced = lex("@{ \$ref }")
        assertTrue(
            "expected ARRAY_VAR `@{ \$ref }`: $spaced",
            has(spaced, StrykeTokenTypes.ARRAY_VAR, "@{ \$ref }"),
        )
    }

    @Test
    fun complex_array_block_deref_does_not_swallow_inner_string() {
        // Regression: `@{ f("{}")->{k} }` used to scan to the FIRST `}` —
        // which lives inside the `"{}"` string literal — swallowing the
        // sigil + half the line into one ARRAY_VAR token and wrecking all
        // downstream highlighting. The interior must now lex as code.
        val src = "@{ GUI::_decode(\"displays\", gui__displays(\"{}\"))->{displays} }"
        val toks = lex(src)
        // No ARRAY_VAR token may start with `@{` — the block-deref opener
        // is emitted as a lone `@` sigil, not a fused variable run.
        assertTrue(
            "no fused `@{...}` ARRAY_VAR token: $toks",
            toks.none { it.first == StrykeTokenTypes.ARRAY_VAR && it.second.startsWith("@{") },
        )
        // The string literals inside the block lex as their own STRING
        // tokens rather than being absorbed into a variable run.
        assertTrue(
            "expected STRING `\"displays\"` inside the block: $toks",
            has(toks, StrykeTokenTypes.STRING, "\"displays\""),
        )
        assertTrue(
            "expected STRING `\"{}\"` inside the block: $toks",
            has(toks, StrykeTokenTypes.STRING, "\"{}\""),
        )
        // The block opener is a lone `@` sigil.
        assertTrue(
            "expected lone `@` sigil opener: $toks",
            has(toks, StrykeTokenTypes.OPERATOR, "@"),
        )
    }
}
