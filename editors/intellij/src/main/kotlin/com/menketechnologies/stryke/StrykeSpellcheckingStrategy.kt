package com.menketechnologies.stryke

import com.intellij.psi.PsiElement
import com.intellij.spellchecker.tokenizer.SpellcheckingStrategy
import com.intellij.spellchecker.tokenizer.Tokenizer

/**
 * Disable the platform's `TypoInspection` for stryke string / comment /
 * heredoc / regex tokens.
 *
 * The default platform behavior is to spell-check every string-literal /
 * comment-like token via the built-in `TextTokenizer`. In a Perl-ish
 * script that flags every non-English identifier (`sub`, `paramsubst`,
 * `bracecomplete`), every path (`Src/glob.c`), every banner divider
 * (`── 2. brace expansion ──`), and every regex pattern as a typo —
 * none of which are typos.
 *
 * Strategy: return `EMPTY_TOKENIZER` for STRING / HEREDOC / COMMENT /
 * DOC_COMMENT / STRING_ESCAPE / STRING_FORMAT / REGEX / REGEX_FLAGS,
 * so the spell-check pass skips them entirely. Identifiers and command
 * names are NOT suppressed — those are where a real typo would matter
 * (`primnt` vs `print`), and the platform's word splitter handles
 * camel/snake cleanly.
 *
 * Port of `zshrs/editors/intellij/.../ZshrsSpellcheckingStrategy.kt`.
 */
class StrykeSpellcheckingStrategy : SpellcheckingStrategy() {
    override fun getTokenizer(element: PsiElement): Tokenizer<*> {
        val node = element.node ?: return super.getTokenizer(element)
        return when (node.elementType) {
            StrykeTokenTypes.STRING,
            StrykeTokenTypes.HEREDOC,
            StrykeTokenTypes.STRING_ESCAPE,
            StrykeTokenTypes.STRING_FORMAT,
            StrykeTokenTypes.COMMENT,
            StrykeTokenTypes.DOC_COMMENT,
            StrykeTokenTypes.REGEX,
            StrykeTokenTypes.REGEX_FLAGS -> EMPTY_TOKENIZER
            else -> super.getTokenizer(element)
        }
    }
}
