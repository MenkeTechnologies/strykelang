package com.menketechnologies.stryke

import com.intellij.codeInsight.editorActions.smartEnter.SmartEnterProcessor
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.ScrollType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile

/**
 * Complete Current Statement (Cmd-Shift-Enter) for stryke.
 *
 * Implements the platform's `lang.smartEnterProcessor` extension by
 * trying a chain of strategies in priority order:
 *
 *  1. **Paren-header block** — `fn` / `method` / `if` / `elsif` /
 *     `while` / `until` / `unless` / `for` / `foreach`. Balances a
 *     missing `)` and appends ` {\n    |\n}`.
 *
 *  2. **Type / impl decl** — `class` / `struct` / `trait` / `enum` /
 *     `impl` (with optional `extends …` / `is …` clause). Appends
 *     ` {\n    |\n}`.
 *
 *  3. **Bare-block keyword** — `else` / `do` / `try` / `catch` /
 *     `finally` / `eval` / `BEGIN` / `END` / `INIT` / `CHECK` /
 *     `UNITCHECK` / `BUILD` / `DESTROY`. Same as type-decl shape,
 *     just no name to skip.
 *
 *  4. **Bracket balance** — current line has an unclosed `(` / `[`
 *     (after skipping string contents). Append the closing chars at
 *     end of line and park caret there.
 *
 * Skipped (return false → platform default Enter): lines where a `{`
 * already follows the header, comment lines, and anything we don't
 * structurally recognise. Returning false lets the platform fall back
 * to its built-in "newline + smart indent" behavior.
 *
 * Logic lives in the companion's [computePlan] so it can be tested
 * without a platform fixture — see [StrykeSmartEnterProcessorTest].
 */
class StrykeSmartEnterProcessor : SmartEnterProcessor() {
    override fun process(project: Project, editor: Editor, file: PsiFile): Boolean {
        if (file.fileType !is StrykeFileType) return false

        val doc = editor.document
        val caret = editor.caretModel.offset
        val text = doc.charsSequence

        val lineNum = doc.getLineNumber(caret)
        val lineStart = doc.getLineStartOffset(lineNum)
        val lineEnd = doc.getLineEndOffset(lineNum)
        val line = text.subSequence(lineStart, lineEnd).toString()

        val plan = computePlan(line, lineStart, caret, text) ?: return false

        // Platform's SmartEnter action wraps `process` in a write
        // command (mirrors XmlSmartEnterProcessor / JsonSmartEnter-
        // Processor in intellij-community), so direct doc mutation
        // is safe here.
        doc.insertString(plan.offset, plan.insert)
        commit(editor)
        editor.caretModel.moveToOffset(plan.offset + plan.caretRel)
        editor.scrollingModel.scrollToCaret(ScrollType.RELATIVE)
        return true
    }

    companion object {
        /** Computed edit: insert [insert] at [offset], caret to [offset]+[caretRel]. */
        data class Plan(val offset: Int, val insert: String, val caretRel: Int)

        /**
         * Pure function: given a stryke source line, return the edit
         * plan that completes its statement, or `null` if no strategy
         * matches. The plan's offsets are absolute (`lineStart`-based)
         * so the caller can apply directly to the document.
         */
        fun computePlan(line: String, lineStart: Int, caret: Int, text: CharSequence): Plan? {
            val trimmed = line.trimStart()
            // Comment line — nothing to do.
            if (trimmed.startsWith("#")) return null

            tryParenHeader(line, lineStart, text)?.let { return it }
            tryDeclOrBlock(line, lineStart, text)?.let { return it }
            tryBracketBalance(line, lineStart, caret)?.let { return it }
            return null
        }

        // ── Strategy 1: header keyword + ( … ) + body ────────────

        private fun tryParenHeader(line: String, lineStart: Int, text: CharSequence): Plan? {
            val keyword = HEADER_KEYWORD.matchAt(line.trimStart(), 0) ?: return null
            val pad = line.length - line.trimStart().length
            val keywordEnd = keyword.range.last + 1 + pad

            val openParen = line.indexOf('(', startIndex = keywordEnd)
            if (openParen < 0) return null

            val closeRel = findMatchingClose(line, openParen, '(', ')')
            val needCloseParen: Boolean
            val closeAbs: Int
            if (closeRel >= 0) {
                needCloseParen = false
                closeAbs = lineStart + closeRel
            } else {
                needCloseParen = true
                closeAbs = lineStart + line.trimEnd().length
            }
            val afterClose = closeAbs + (if (needCloseParen) 0 else 1)
            if (hasBraceAfter(text, afterClose)) return null

            return blockInsertPlan(afterClose, leadingIndent(line), if (needCloseParen) ")" else "")
        }

        // ── Strategy 2: type/decl keyword OR bare-block keyword ──

        private fun tryDeclOrBlock(line: String, lineStart: Int, text: CharSequence): Plan? {
            val trimmed = line.trimStart()
            // Both DECL and BARE share the "keyword … {" shape;
            // they're separated only by which keywords trigger them.
            val match = DECL_KEYWORD.matchAt(trimmed, 0)
                ?: BARE_BLOCK_KEYWORD.matchAt(trimmed, 0)
                ?: return null

            // If the line still has an unclosed paren, leave it to
            // the paren-header strategy on the next invocation — we
            // don't want to insert a brace block before the user's
            // header is parens-complete.
            val firstOpen = line.indexOf('(')
            if (firstOpen >= 0 && findMatchingClose(line, firstOpen, '(', ')') < 0) return null

            // `class Foo;`, `do BLOCK;` etc. — Perl/stryke statement
            // terminators after the keyword body indicate the user
            // already intends no block. Bail.
            if (trimmed.trimEnd().endsWith(";")) return null

            // Body already started on this line (`class Foo {`) or on
            // the following line — leave it alone.
            if (lineHasOpenBrace(line)) return null
            val afterLine = lineStart + line.trimEnd().length
            if (hasBraceAfter(text, afterLine)) return null

            // Ignore prepositions: a `class Foo` is ready for a body,
            // but `class Foo extends` (cursor in mid-clause) shouldn't
            // get a body slammed on yet — `extends` needs a parent
            // name.
            val tail = trimmed.substring(match.range.last + 1).trimStart()
            if (DECL_INCOMPLETE_TAIL.containsMatchIn(tail)) return null

            return blockInsertPlan(afterLine, leadingIndent(line), prefix = "")
        }

        // ── Strategy 3: balance unclosed `(` / `[` on the line ───

        private fun tryBracketBalance(line: String, lineStart: Int, caret: Int): Plan? {
            // Scan the line tracking paren / bracket depth, skipping
            // over string contents so quotes around a bracket don't
            // mis-count. If the line ends with positive depth on
            // either, close them in the right order.
            val parens = ArrayDeque<Char>()
            var i = 0
            while (i < line.length) {
                val c = line[i]
                when (c) {
                    '(' -> parens.addLast(')')
                    '[' -> parens.addLast(']')
                    '{' -> parens.addLast('}')
                    ')', ']', '}' -> if (parens.lastOrNull() == c) parens.removeLast()
                    '"', '\'' -> {
                        val q = c
                        i++
                        while (i < line.length && line[i] != q) {
                            if (line[i] == '\\' && i + 1 < line.length) i++
                            i++
                        }
                    }
                    '#' -> break // rest of line is a comment
                }
                i++
            }
            if (parens.isEmpty()) return null

            // Don't insert a `}` here — that's the block strategy's
            // job. Only auto-close `)` and `]` runs.
            val closers = parens.reversed().joinToString("") { it.toString() }
            if (closers.any { it == '}' }) return null

            val endAbs = lineStart + line.trimEnd().length
            return Plan(offset = endAbs, insert = closers, caretRel = closers.length)
        }

        // ── Shared plumbing ──────────────────────────────────────

        /**
         * Build a ` PREFIX {\n<indent>    \n<indent>}` insertion. Caret
         * lands at the end of the body indent. [prefix] is normally
         * empty; pass `")"` to also close a missing parameter paren.
         */
        private fun blockInsertPlan(afterAnchor: Int, indent: String, prefix: String): Plan {
            val insert = buildString {
                append(prefix)
                append(" {\n")
                append(indent)
                append("    ")
                append("\n")
                append(indent)
                append('}')
            }
            // Caret = afterAnchor + prefix.length + " {\n".length(3) +
            //         indent.length + 4 (body indent past leading).
            val caretRel = prefix.length + 3 + indent.length + 4
            return Plan(offset = afterAnchor, insert = insert, caretRel = caretRel)
        }

        /** Find the matching close char for [open] at [openIdx]. -1 if unbalanced. */
        private fun findMatchingClose(line: String, openIdx: Int, open: Char, close: Char): Int {
            var depth = 0
            var i = openIdx
            while (i < line.length) {
                val c = line[i]
                when {
                    c == open -> depth++
                    c == close -> {
                        depth--
                        if (depth == 0) return i
                    }
                    c == '"' || c == '\'' -> {
                        val q = c
                        i++
                        while (i < line.length && line[i] != q) {
                            if (line[i] == '\\' && i + 1 < line.length) i++
                            i++
                        }
                    }
                    c == '#' -> return -1 // rest is comment
                }
                i++
            }
            return -1
        }

        /**
         * True when [line] already contains an unmatched `{` outside
         * string literals and comments — meaning the user has begun
         * the body on this same line and we must not append another.
         */
        private fun lineHasOpenBrace(line: String): Boolean {
            var i = 0
            while (i < line.length) {
                val c = line[i]
                when (c) {
                    '{' -> return true
                    '"', '\'' -> {
                        val q = c
                        i++
                        while (i < line.length && line[i] != q) {
                            if (line[i] == '\\' && i + 1 < line.length) i++
                            i++
                        }
                    }
                    '#' -> return false
                }
                i++
            }
            return false
        }

        /** True when the chars after [offset] are whitespace then `{`. */
        private fun hasBraceAfter(text: CharSequence, offset: Int): Boolean {
            var i = offset
            while (i < text.length) {
                val c = text[i]
                if (c == '{') return true
                if (c != ' ' && c != '\t' && c != '\n' && c != '\r') return false
                i++
            }
            return false
        }

        private fun leadingIndent(line: String): String {
            val end = line.indexOfFirst { it != ' ' && it != '\t' }
            return if (end < 0) line else line.substring(0, end)
        }

        // ── Keyword tables ──────────────────────────────────────

        /** Keywords whose header takes a `(...)` paren clause. */
        private val HEADER_KEYWORD = Regex(
            "(fn|method|if|elsif|while|until|unless|for|foreach)\\b",
        )

        /** Type-shaped decls: `class`, `struct`, `trait`, `enum`, `impl`. */
        private val DECL_KEYWORD = Regex(
            "(class|struct|trait|enum|impl)\\b",
        )

        /** Body-only blocks (no header). */
        private val BARE_BLOCK_KEYWORD = Regex(
            "(else|do|try|catch|finally|eval|BEGIN|END|INIT|CHECK|UNITCHECK|BUILD|DESTROY)\\b",
        )

        /**
         * After a type-decl keyword, these trailing tokens signal an
         * incomplete clause (`extends`, `is`, `as`, `where`) where the
         * user is mid-typing a parent / trait / constraint and the
         * body isn't ready yet. Without this, `class Foo extends`
         * would get a body slammed on before `extends` got its
         * argument.
         */
        private val DECL_INCOMPLETE_TAIL = Regex(
            "\\b(extends|is|as|where|implements|impl)\\b\\s*\$",
        )
    }
}
