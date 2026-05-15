package com.menketechnologies.stryke

import com.intellij.lang.Language

object StrykeLanguage : Language("Stryke") {
    private fun readResolve(): Any = StrykeLanguage
    override fun getDisplayName(): String = "Stryke"
    override fun isCaseSensitive(): Boolean = true
}
