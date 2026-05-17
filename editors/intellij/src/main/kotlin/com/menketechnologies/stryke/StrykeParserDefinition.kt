package com.menketechnologies.stryke

import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiBuilder
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.TokenType
import com.intellij.psi.impl.source.tree.LeafPsiElement
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import com.intellij.testFramework.LightVirtualFile
import com.intellij.openapi.fileTypes.FileType

/**
 * Minimal parser definition for `.stk` files. Provides IntelliJ with a
 * real `PsiFile` for stryke source, which is what unlocks
 *
 *   * `Cmd-/` line-comment toggle (handler looks up the language via
 *     the PsiElement at the caret),
 *   * Extract refactorings (Extract Variable / Constant / Function)
 *     surfaced via the LSP — IntelliJ anchors their edits to the PSI,
 *   * brace-matcher cursor highlighting,
 *   * structure view, find-usages, etc.
 *
 * We don't ship a full recursive-descent parser here — the stryke LSP
 * already does that for diagnostics, semantic tokens, refactorings, and
 * folding. The PSI is intentionally flat: every lexer token becomes a
 * leaf node. That's enough for everything above, while the heavy lifting
 * still happens server-side.
 */
class StrykeParserDefinition : ParserDefinition {
    override fun createLexer(project: Project?): Lexer = StrykeLexer()

    override fun createParser(project: Project?): PsiParser = StrykeFlatParser()

    override fun getFileNodeType(): IFileElementType = FILE

    override fun getCommentTokens(): TokenSet =
        TokenSet.create(StrykeTokenTypes.COMMENT, StrykeTokenTypes.DOC_COMMENT)

    override fun getStringLiteralElements(): TokenSet =
        TokenSet.create(StrykeTokenTypes.STRING, StrykeTokenTypes.HEREDOC)

    override fun createFile(viewProvider: FileViewProvider): PsiFile = StrykePsiFile(viewProvider)

    override fun createElement(node: ASTNode): PsiElement = LeafPsiElement(node.elementType, node.text)

    companion object {
        val FILE: IFileElementType = IFileElementType("STRYKE_FILE", StrykeLanguage)
    }
}

/**
 * Flat parser: consumes every lexer token and emits it as a top-level
 * sibling. We never construct nested AST nodes — IntelliJ doesn't need
 * them for the features we want (comment toggle, refactorings via LSP,
 * brace match, structure-view via document symbols).
 */
private class StrykeFlatParser : PsiParser {
    override fun parse(root: com.intellij.psi.tree.IElementType, builder: PsiBuilder): ASTNode {
        val rootMarker = builder.mark()
        while (!builder.eof()) {
            builder.advanceLexer()
        }
        rootMarker.done(root)
        return builder.treeBuilt
    }
}

/**
 * PsiFile backing a `.stk` document. The IntelliJ framework needs a
 * concrete subclass before the language is "fully realised"; without
 * one, the line-comment action and several refactoring entry points
 * silently no-op because they bail before reaching the commenter.
 */
class StrykePsiFile(viewProvider: FileViewProvider) :
    com.intellij.extapi.psi.PsiFileBase(viewProvider, StrykeLanguage) {
    override fun getFileType(): FileType = StrykeFileType
    override fun toString(): String = "Stryke File"
}
