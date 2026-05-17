package com.menketechnologies.stryke

import com.intellij.codeInsight.editorActions.QuoteHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.highlighter.HighlighterIterator
import com.intellij.psi.tree.IElementType

/**
 * Auto-pair `"`, `'`, and `` ` `` in stryke source.
 *
 * We don't use [com.intellij.codeInsight.editorActions.SimpleTokenSetQuoteHandler]
 * because stryke's `"..."` strings now lex into multiple sub-tokens
 * (literal-STRING / OPERATOR `#{` / interior code tokens / OPERATOR `}` /
 * literal-STRING) so the simple "is the token at the cursor a STRING?"
 * check answers no for interior positions and breaks the standard
 * "inside literal → skip close" path.
 *
 * Instead, we use a char-only handler that scans the document text
 * directly. This is more robust against lexer state changes during
 * incremental editing and matches how Perl / Python / similar plugins
 * implement quote pairing in JetBrains IDEs.
 */
class StrykeQuoteHandler : QuoteHandler {
    override fun isClosingQuote(iterator: HighlighterIterator, offset: Int): Boolean {
        val ch = charAt(iterator, offset) ?: return false
        if (!isQuoteChar(ch)) return false
        // Walk backwards from `offset` along the same line; if we find an
        // unescaped matching quote, the one at `offset` is its closer.
        return matchingOpenBefore(iterator, offset, ch)
    }

    override fun isOpeningQuote(iterator: HighlighterIterator, offset: Int): Boolean {
        val ch = charAt(iterator, offset) ?: return false
        if (!isQuoteChar(ch)) return false
        // Opening = NOT closing. Inverse of isClosingQuote.
        return !matchingOpenBefore(iterator, offset, ch)
    }

    override fun hasNonClosedLiteral(
        editor: Editor,
        iterator: HighlighterIterator,
        offset: Int,
    ): Boolean {
        // Always claim an unclosed literal so the platform inserts the
        // matching close quote. Without this, typing `"` at end-of-line
        // doesn't auto-pair on some IntelliJ versions.
        return true
    }

    override fun isInsideLiteral(iterator: HighlighterIterator): Boolean {
        val tt: IElementType? = iterator.tokenType
        return tt == StrykeTokenTypes.STRING ||
                tt == StrykeTokenTypes.HEREDOC ||
                tt == StrykeTokenTypes.STRING_ESCAPE
    }

    private fun isQuoteChar(c: Char): Boolean = c == '"' || c == '\'' || c == '`'

    private fun charAt(iterator: HighlighterIterator, offset: Int): Char? {
        val doc = iterator.document ?: return null
        if (offset < 0 || offset >= doc.textLength) return null
        return doc.charsSequence[offset]
    }

    /**
     * Walk left from `offset` along the same line. If we encounter an
     * unescaped `quote` char before hitting a line boundary, the one at
     * `offset` is closing the literal opened there.
     */
    private fun matchingOpenBefore(
        iterator: HighlighterIterator,
        offset: Int,
        quote: Char,
    ): Boolean {
        val doc = iterator.document ?: return false
        val text = doc.charsSequence
        var i = offset - 1
        while (i >= 0) {
            val c = text[i]
            if (c == '\n') return false
            if (c == quote && !isEscaped(text, i)) {
                return true
            }
            i--
        }
        return false
    }

    private fun isEscaped(text: CharSequence, idx: Int): Boolean {
        var n = 0
        var i = idx - 1
        while (i >= 0 && text[i] == '\\') {
            n++; i--
        }
        return n % 2 == 1
    }
}
