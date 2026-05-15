package com.menketechnologies.stryke

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.HighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.openapi.fileTypes.SyntaxHighlighterFactory
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

class StrykeSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = StrykeLexer()

    override fun getTokenHighlights(type: IElementType): Array<TextAttributesKey> = when (type) {
        StrykeTokenTypes.COMMENT -> pack(DefaultLanguageHighlighterColors.LINE_COMMENT)
        StrykeTokenTypes.STRING -> pack(DefaultLanguageHighlighterColors.STRING)
        StrykeTokenTypes.NUMBER -> pack(DefaultLanguageHighlighterColors.NUMBER)
        StrykeTokenTypes.KEYWORD -> pack(DefaultLanguageHighlighterColors.KEYWORD)
        StrykeTokenTypes.BUILTIN -> pack(DefaultLanguageHighlighterColors.STATIC_METHOD)
        StrykeTokenTypes.SCALAR_VAR -> pack(DefaultLanguageHighlighterColors.LOCAL_VARIABLE)
        StrykeTokenTypes.ARRAY_VAR -> pack(DefaultLanguageHighlighterColors.GLOBAL_VARIABLE)
        StrykeTokenTypes.HASH_VAR -> pack(DefaultLanguageHighlighterColors.INSTANCE_FIELD)
        StrykeTokenTypes.OPERATOR -> pack(DefaultLanguageHighlighterColors.OPERATION_SIGN)
        StrykeTokenTypes.PIPE -> pack(DefaultLanguageHighlighterColors.LABEL)
        StrykeTokenTypes.REGEX -> pack(DefaultLanguageHighlighterColors.MARKUP_ATTRIBUTE)
        StrykeTokenTypes.IDENTIFIER -> pack(DefaultLanguageHighlighterColors.IDENTIFIER)
        TokenType.BAD_CHARACTER -> pack(HighlighterColors.BAD_CHARACTER)
        else -> emptyArray()
    }

    private fun pack(key: TextAttributesKey): Array<TextAttributesKey> = arrayOf(key)
}

class StrykeSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        StrykeSyntaxHighlighter()
}
