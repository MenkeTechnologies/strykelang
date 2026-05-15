package com.menketechnologies.stryke

import com.intellij.psi.tree.IElementType

class StrykeTokenType(debugName: String) : IElementType(debugName, StrykeLanguage)

object StrykeTokenTypes {
    @JvmField val COMMENT = StrykeTokenType("STRYKE_COMMENT")
    @JvmField val STRING = StrykeTokenType("STRYKE_STRING")
    @JvmField val NUMBER = StrykeTokenType("STRYKE_NUMBER")
    @JvmField val KEYWORD = StrykeTokenType("STRYKE_KEYWORD")
    @JvmField val BUILTIN = StrykeTokenType("STRYKE_BUILTIN")
    @JvmField val SCALAR_VAR = StrykeTokenType("STRYKE_SCALAR_VAR")
    @JvmField val ARRAY_VAR = StrykeTokenType("STRYKE_ARRAY_VAR")
    @JvmField val HASH_VAR = StrykeTokenType("STRYKE_HASH_VAR")
    @JvmField val OPERATOR = StrykeTokenType("STRYKE_OPERATOR")
    @JvmField val IDENTIFIER = StrykeTokenType("STRYKE_IDENTIFIER")
    @JvmField val REGEX = StrykeTokenType("STRYKE_REGEX")
    @JvmField val PIPE = StrykeTokenType("STRYKE_PIPE")
    @JvmField val BAD = StrykeTokenType("STRYKE_BAD")
}
