package com.menketechnologies.stryke

import com.intellij.psi.tree.IElementType

class StrykeTokenType(debugName: String) : IElementType(debugName, StrykeLanguage)

/**
 * Fine-grained token types — each maps to its own [StrykeColors] entry so the
 * user can recolor any category independently. When adding a token type here,
 * also add the matching case in [StrykeSyntaxHighlighter.getTokenHighlights]
 * and the matching entry in [StrykeColorSettingsPage.attrs].
 */
object StrykeTokenTypes {
    // ── Trivia / literals ──────────────────────────────────────────────
    @JvmField val COMMENT = StrykeTokenType("STRYKE_COMMENT")
    @JvmField val DOC_COMMENT = StrykeTokenType("STRYKE_DOC_COMMENT")
    @JvmField val STRING = StrykeTokenType("STRYKE_STRING")
    @JvmField val HEREDOC = StrykeTokenType("STRYKE_HEREDOC")
    @JvmField val STRING_ESCAPE = StrykeTokenType("STRYKE_STRING_ESCAPE")
    @JvmField val STRING_FORMAT = StrykeTokenType("STRYKE_STRING_FORMAT")  // printf-style %d %s %10.2f
    @JvmField val NUMBER = StrykeTokenType("STRYKE_NUMBER")
    @JvmField val FLOAT = StrykeTokenType("STRYKE_FLOAT")
    @JvmField val REGEX = StrykeTokenType("STRYKE_REGEX")
    @JvmField val REGEX_FLAGS = StrykeTokenType("STRYKE_REGEX_FLAGS")

    // ── Keywords ──────────────────────────────────────────────────────
    @JvmField val KEYWORD = StrykeTokenType("STRYKE_KEYWORD")
    @JvmField val DECL_KEYWORD = StrykeTokenType("STRYKE_DECL_KEYWORD")   // my our local state use no
    @JvmField val FN_KEYWORD = StrykeTokenType("STRYKE_FN_KEYWORD")        // fn sub class trait struct enum
    @JvmField val CONTROL_KEYWORD = StrykeTokenType("STRYKE_CONTROL_KEYWORD") // if else while for return
    @JvmField val PHASE_KEYWORD = StrykeTokenType("STRYKE_PHASE_KEYWORD")  // BEGIN END INIT CHECK UNITCHECK
    @JvmField val WORD_OPERATOR = StrykeTokenType("STRYKE_WORD_OPERATOR")  // and or not xor eq ne lt cmp x
    @JvmField val BOOLEAN = StrykeTokenType("STRYKE_BOOLEAN")              // true false
    @JvmField val UNDEF = StrykeTokenType("STRYKE_UNDEF")                  // undef

    // ── Identifiers / names ──────────────────────────────────────────────
    @JvmField val BUILTIN = StrykeTokenType("STRYKE_BUILTIN")
    @JvmField val BUILTIN_PARALLEL = StrykeTokenType("STRYKE_BUILTIN_PARALLEL") // pmap pgrep pfor etc.
    @JvmField val FUNCTION_CALL = StrykeTokenType("STRYKE_FUNCTION_CALL")
    @JvmField val FUNCTION_DECL = StrykeTokenType("STRYKE_FUNCTION_DECL")
    @JvmField val IDENTIFIER = StrykeTokenType("STRYKE_IDENTIFIER")
    @JvmField val PACKAGE_NAME = StrykeTokenType("STRYKE_PACKAGE_NAME")
    @JvmField val PACKAGE_SEPARATOR = StrykeTokenType("STRYKE_PACKAGE_SEPARATOR") // ::
    @JvmField val LABEL = StrykeTokenType("STRYKE_LABEL")

    // ── Variables ─────────────────────────────────────────────────────────
    @JvmField val SIGIL = StrykeTokenType("STRYKE_SIGIL")
    @JvmField val SCALAR_VAR = StrykeTokenType("STRYKE_SCALAR_VAR")
    @JvmField val ARRAY_VAR = StrykeTokenType("STRYKE_ARRAY_VAR")
    @JvmField val HASH_VAR = StrykeTokenType("STRYKE_HASH_VAR")
    @JvmField val SPECIAL_VAR = StrykeTokenType("STRYKE_SPECIAL_VAR")     // $! $@ $/ $, etc.
    @JvmField val TOPIC_VAR = StrykeTokenType("STRYKE_TOPIC_VAR")         // $_ @_ _
    @JvmField val BLOCK_PARAM = StrykeTokenType("STRYKE_BLOCK_PARAM")     // _0 _1 _N $_0 etc.

    // ── Operators ────────────────────────────────────────────────────────
    @JvmField val OPERATOR = StrykeTokenType("STRYKE_OPERATOR")
    @JvmField val ASSIGN_OP = StrykeTokenType("STRYKE_ASSIGN_OP")          // = += -= etc.
    @JvmField val ARROW_OP = StrykeTokenType("STRYKE_ARROW_OP")            // ->
    @JvmField val FAT_COMMA = StrykeTokenType("STRYKE_FAT_COMMA")          // =>
    @JvmField val PIPE = StrykeTokenType("STRYKE_PIPE")                    // |> ~> |>>
    @JvmField val RANGE = StrykeTokenType("STRYKE_RANGE")                  // .. : in range context
    @JvmField val REGEX_BIND = StrykeTokenType("STRYKE_REGEX_BIND")        // =~ !~

    // ── Punctuation ──────────────────────────────────────────────────────
    // Split L/R variants so the BraceMatcher can pair them; the original
    // `PAREN`/`BRACE`/`BRACKET` umbrella names stay for syntax-highlighter
    // compatibility (any of the four colors below land on the same scheme
    // slot through StrykeSyntaxHighlighter).
    @JvmField val PAREN = StrykeTokenType("STRYKE_PAREN")
    @JvmField val LPAREN = StrykeTokenType("STRYKE_LPAREN")
    @JvmField val RPAREN = StrykeTokenType("STRYKE_RPAREN")
    @JvmField val BRACE = StrykeTokenType("STRYKE_BRACE")
    @JvmField val LBRACE = StrykeTokenType("STRYKE_LBRACE")
    @JvmField val RBRACE = StrykeTokenType("STRYKE_RBRACE")
    @JvmField val BRACKET = StrykeTokenType("STRYKE_BRACKET")
    @JvmField val LBRACKET = StrykeTokenType("STRYKE_LBRACKET")
    @JvmField val RBRACKET = StrykeTokenType("STRYKE_RBRACKET")
    @JvmField val COMMA = StrykeTokenType("STRYKE_COMMA")
    @JvmField val SEMICOLON = StrykeTokenType("STRYKE_SEMICOLON")
    @JvmField val DOT = StrykeTokenType("STRYKE_DOT")

    // ── Errors ───────────────────────────────────────────────────────────
    @JvmField val BAD = StrykeTokenType("STRYKE_BAD")
}
