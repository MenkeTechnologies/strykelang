package com.menketechnologies.stryke

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as Defaults
import com.intellij.openapi.editor.HighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey

/**
 * Stable, plugin-owned [TextAttributesKey]s for every stryke token category.
 *
 * Each key inherits a sensible default from [Defaults] but lives in its own
 * namespace (`STRYKE_*`) so users can rebind any of them in
 * *Settings → Editor → Color Scheme → Stryke* without affecting the rest of
 * the IDE. Add new categories here, not inline in the highlighter.
 */
object StrykeColors {
    @JvmField val COMMENT = mk("STRYKE_COMMENT", Defaults.LINE_COMMENT)
    @JvmField val DOC_COMMENT = mk("STRYKE_DOC_COMMENT", Defaults.DOC_COMMENT)
    @JvmField val STRING = mk("STRYKE_STRING", Defaults.STRING)
    @JvmField val STRING_ESCAPE = mk("STRYKE_STRING_ESCAPE", Defaults.VALID_STRING_ESCAPE)
    @JvmField val STRING_FORMAT = mk("STRYKE_STRING_FORMAT", Defaults.VALID_STRING_ESCAPE)
    @JvmField val HEREDOC = mk("STRYKE_HEREDOC", Defaults.STRING)
    @JvmField val NUMBER = mk("STRYKE_NUMBER", Defaults.NUMBER)
    @JvmField val FLOAT = mk("STRYKE_FLOAT", Defaults.NUMBER)
    @JvmField val REGEX = mk("STRYKE_REGEX", Defaults.MARKUP_ATTRIBUTE)
    @JvmField val REGEX_FLAGS = mk("STRYKE_REGEX_FLAGS", Defaults.MARKUP_TAG)

    @JvmField val KEYWORD = mk("STRYKE_KEYWORD", Defaults.KEYWORD)
    @JvmField val DECL_KEYWORD = mk("STRYKE_DECL_KEYWORD", Defaults.KEYWORD)
    @JvmField val FN_KEYWORD = mk("STRYKE_FN_KEYWORD", Defaults.KEYWORD)
    @JvmField val CONTROL_KEYWORD = mk("STRYKE_CONTROL_KEYWORD", Defaults.KEYWORD)
    @JvmField val PHASE_KEYWORD = mk("STRYKE_PHASE_KEYWORD", Defaults.METADATA)
    @JvmField val WORD_OPERATOR = mk("STRYKE_WORD_OPERATOR", Defaults.OPERATION_SIGN)
    @JvmField val BOOLEAN = mk("STRYKE_BOOLEAN", Defaults.PREDEFINED_SYMBOL)
    @JvmField val UNDEF = mk("STRYKE_UNDEF", Defaults.PREDEFINED_SYMBOL)

    @JvmField val BUILTIN = mk("STRYKE_BUILTIN", Defaults.STATIC_METHOD)
    @JvmField val BUILTIN_PARALLEL = mk("STRYKE_BUILTIN_PARALLEL", Defaults.STATIC_METHOD)
    @JvmField val FUNCTION_CALL = mk("STRYKE_FUNCTION_CALL", Defaults.FUNCTION_CALL)
    @JvmField val FUNCTION_DECL = mk("STRYKE_FUNCTION_DECL", Defaults.FUNCTION_DECLARATION)
    @JvmField val IDENTIFIER = mk("STRYKE_IDENTIFIER", Defaults.IDENTIFIER)
    @JvmField val PACKAGE_NAME = mk("STRYKE_PACKAGE_NAME", Defaults.CLASS_NAME)
    @JvmField val PACKAGE_SEPARATOR = mk("STRYKE_PACKAGE_SEPARATOR", Defaults.DOT)

    @JvmField val SIGIL = mk("STRYKE_SIGIL", Defaults.OPERATION_SIGN)
    @JvmField val SCALAR_VAR = mk("STRYKE_SCALAR_VAR", Defaults.LOCAL_VARIABLE)
    @JvmField val ARRAY_VAR = mk("STRYKE_ARRAY_VAR", Defaults.GLOBAL_VARIABLE)
    @JvmField val HASH_VAR = mk("STRYKE_HASH_VAR", Defaults.INSTANCE_FIELD)
    @JvmField val SPECIAL_VAR = mk("STRYKE_SPECIAL_VAR", Defaults.PREDEFINED_SYMBOL)
    @JvmField val TOPIC_VAR = mk("STRYKE_TOPIC_VAR", Defaults.PREDEFINED_SYMBOL)
    @JvmField val BLOCK_PARAM = mk("STRYKE_BLOCK_PARAM", Defaults.PARAMETER)
    @JvmField val PARAMETER = mk("STRYKE_PARAMETER", Defaults.PARAMETER)
    @JvmField val LABEL = mk("STRYKE_LABEL", Defaults.LABEL)

    @JvmField val OPERATOR = mk("STRYKE_OPERATOR", Defaults.OPERATION_SIGN)
    @JvmField val ASSIGN_OP = mk("STRYKE_ASSIGN_OP", Defaults.OPERATION_SIGN)
    @JvmField val ARROW_OP = mk("STRYKE_ARROW_OP", Defaults.DOT)
    @JvmField val FAT_COMMA = mk("STRYKE_FAT_COMMA", Defaults.DOT)
    @JvmField val PIPE = mk("STRYKE_PIPE", Defaults.LABEL)
    @JvmField val RANGE = mk("STRYKE_RANGE", Defaults.OPERATION_SIGN)
    @JvmField val REGEX_BIND = mk("STRYKE_REGEX_BIND", Defaults.OPERATION_SIGN)

    @JvmField val PAREN = mk("STRYKE_PAREN", Defaults.PARENTHESES)
    @JvmField val BRACE = mk("STRYKE_BRACE", Defaults.BRACES)
    @JvmField val BRACKET = mk("STRYKE_BRACKET", Defaults.BRACKETS)
    @JvmField val COMMA = mk("STRYKE_COMMA", Defaults.COMMA)
    @JvmField val SEMICOLON = mk("STRYKE_SEMICOLON", Defaults.SEMICOLON)
    @JvmField val DOT = mk("STRYKE_DOT", Defaults.DOT)

    @JvmField val BAD_CHAR = mk("STRYKE_BAD_CHAR", HighlighterColors.BAD_CHARACTER)

    private fun mk(name: String, fallback: TextAttributesKey): TextAttributesKey =
        TextAttributesKey.createTextAttributesKey(name, fallback)
}
