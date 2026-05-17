package com.menketechnologies.stryke

import com.intellij.lang.PairedBraceMatcher
import com.intellij.lang.BracePair
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType

/**
 * Brace pairing for stryke. Powers auto-insertion of the matching
 * `)` / `}` / `]` when you type `(` / `{` / `[`, AND structural brace
 * highlighting when the cursor is next to a paired delimiter.
 */
class StrykeBraceMatcher : PairedBraceMatcher {
    private val pairs = arrayOf(
        BracePair(StrykeTokenTypes.LPAREN, StrykeTokenTypes.RPAREN, false),
        BracePair(StrykeTokenTypes.LBRACE, StrykeTokenTypes.RBRACE, true),
        BracePair(StrykeTokenTypes.LBRACKET, StrykeTokenTypes.RBRACKET, false),
    )

    override fun getPairs(): Array<BracePair> = pairs

    override fun isPairedBracesAllowedBeforeType(
        lbraceType: IElementType,
        contextType: IElementType?,
    ): Boolean = true

    override fun getCodeConstructStart(file: PsiFile?, openingBraceOffset: Int): Int =
        openingBraceOffset
}
