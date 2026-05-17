package com.menketechnologies.stryke

import com.intellij.lexer.Lexer
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

    override fun getTokenHighlights(type: IElementType): Array<TextAttributesKey> {
        val key: TextAttributesKey? = when (type) {
            StrykeTokenTypes.COMMENT -> StrykeColors.COMMENT
            StrykeTokenTypes.DOC_COMMENT -> StrykeColors.DOC_COMMENT
            StrykeTokenTypes.STRING -> StrykeColors.STRING
            StrykeTokenTypes.STRING_ESCAPE -> StrykeColors.STRING_ESCAPE
            StrykeTokenTypes.HEREDOC -> StrykeColors.HEREDOC
            StrykeTokenTypes.NUMBER -> StrykeColors.NUMBER
            StrykeTokenTypes.FLOAT -> StrykeColors.FLOAT
            StrykeTokenTypes.REGEX -> StrykeColors.REGEX
            StrykeTokenTypes.REGEX_FLAGS -> StrykeColors.REGEX_FLAGS

            StrykeTokenTypes.KEYWORD -> StrykeColors.KEYWORD
            StrykeTokenTypes.DECL_KEYWORD -> StrykeColors.DECL_KEYWORD
            StrykeTokenTypes.FN_KEYWORD -> StrykeColors.FN_KEYWORD
            StrykeTokenTypes.CONTROL_KEYWORD -> StrykeColors.CONTROL_KEYWORD
            StrykeTokenTypes.PHASE_KEYWORD -> StrykeColors.PHASE_KEYWORD
            StrykeTokenTypes.WORD_OPERATOR -> StrykeColors.WORD_OPERATOR
            StrykeTokenTypes.BOOLEAN -> StrykeColors.BOOLEAN
            StrykeTokenTypes.UNDEF -> StrykeColors.UNDEF

            StrykeTokenTypes.BUILTIN -> StrykeColors.BUILTIN
            StrykeTokenTypes.BUILTIN_PARALLEL -> StrykeColors.BUILTIN_PARALLEL
            StrykeTokenTypes.FUNCTION_CALL -> StrykeColors.FUNCTION_CALL
            StrykeTokenTypes.FUNCTION_DECL -> StrykeColors.FUNCTION_DECL
            StrykeTokenTypes.IDENTIFIER -> StrykeColors.IDENTIFIER
            StrykeTokenTypes.PACKAGE_NAME -> StrykeColors.PACKAGE_NAME
            StrykeTokenTypes.PACKAGE_SEPARATOR -> StrykeColors.PACKAGE_SEPARATOR
            StrykeTokenTypes.LABEL -> StrykeColors.LABEL

            StrykeTokenTypes.SIGIL -> StrykeColors.SIGIL
            StrykeTokenTypes.SCALAR_VAR -> StrykeColors.SCALAR_VAR
            StrykeTokenTypes.ARRAY_VAR -> StrykeColors.ARRAY_VAR
            StrykeTokenTypes.HASH_VAR -> StrykeColors.HASH_VAR
            StrykeTokenTypes.SPECIAL_VAR -> StrykeColors.SPECIAL_VAR
            StrykeTokenTypes.TOPIC_VAR -> StrykeColors.TOPIC_VAR
            StrykeTokenTypes.BLOCK_PARAM -> StrykeColors.BLOCK_PARAM

            StrykeTokenTypes.OPERATOR -> StrykeColors.OPERATOR
            StrykeTokenTypes.ASSIGN_OP -> StrykeColors.ASSIGN_OP
            StrykeTokenTypes.ARROW_OP -> StrykeColors.ARROW_OP
            StrykeTokenTypes.FAT_COMMA -> StrykeColors.FAT_COMMA
            StrykeTokenTypes.PIPE -> StrykeColors.PIPE
            StrykeTokenTypes.RANGE -> StrykeColors.RANGE
            StrykeTokenTypes.REGEX_BIND -> StrykeColors.REGEX_BIND

            StrykeTokenTypes.PAREN -> StrykeColors.PAREN
            StrykeTokenTypes.LPAREN -> StrykeColors.PAREN
            StrykeTokenTypes.RPAREN -> StrykeColors.PAREN
            StrykeTokenTypes.BRACE -> StrykeColors.BRACE
            StrykeTokenTypes.LBRACE -> StrykeColors.BRACE
            StrykeTokenTypes.RBRACE -> StrykeColors.BRACE
            StrykeTokenTypes.BRACKET -> StrykeColors.BRACKET
            StrykeTokenTypes.LBRACKET -> StrykeColors.BRACKET
            StrykeTokenTypes.RBRACKET -> StrykeColors.BRACKET
            StrykeTokenTypes.COMMA -> StrykeColors.COMMA
            StrykeTokenTypes.SEMICOLON -> StrykeColors.SEMICOLON
            StrykeTokenTypes.DOT -> StrykeColors.DOT

            TokenType.BAD_CHARACTER -> StrykeColors.BAD_CHAR
            else -> null
        }
        return if (key == null) emptyArray() else arrayOf(key)
    }
}

class StrykeSyntaxHighlighterFactory : SyntaxHighlighterFactory() {
    override fun getSyntaxHighlighter(project: Project?, virtualFile: VirtualFile?): SyntaxHighlighter =
        StrykeSyntaxHighlighter()
}
