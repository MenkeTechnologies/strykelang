package com.menketechnologies.stryke

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

object StrykeFileType : LanguageFileType(StrykeLanguage) {
    override fun getName(): String = "Stryke"
    override fun getDescription(): String = "Stryke programming language"
    override fun getDefaultExtension(): String = "stk"
    override fun getIcon(): Icon = StrykeIcons.FILE
}
