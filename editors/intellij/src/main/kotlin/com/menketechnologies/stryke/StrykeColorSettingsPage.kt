package com.menketechnologies.stryke

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import javax.swing.Icon

class StrykeColorSettingsPage : ColorSettingsPage {
    private val attrs = arrayOf(
        AttributesDescriptor("Comment", DefaultLanguageHighlighterColors.LINE_COMMENT),
        AttributesDescriptor("String", DefaultLanguageHighlighterColors.STRING),
        AttributesDescriptor("Number", DefaultLanguageHighlighterColors.NUMBER),
        AttributesDescriptor("Keyword", DefaultLanguageHighlighterColors.KEYWORD),
        AttributesDescriptor("Builtin", DefaultLanguageHighlighterColors.STATIC_METHOD),
        AttributesDescriptor("Scalar \$var", DefaultLanguageHighlighterColors.LOCAL_VARIABLE),
        AttributesDescriptor("Array @var", DefaultLanguageHighlighterColors.GLOBAL_VARIABLE),
        AttributesDescriptor("Hash %var", DefaultLanguageHighlighterColors.INSTANCE_FIELD),
        AttributesDescriptor("Operator", DefaultLanguageHighlighterColors.OPERATION_SIGN),
        AttributesDescriptor("Pipe |> / ~>", DefaultLanguageHighlighterColors.LABEL),
        AttributesDescriptor("Regex", DefaultLanguageHighlighterColors.MARKUP_ATTRIBUTE),
    )

    override fun getIcon(): Icon = StrykeIcons.FILE
    override fun getHighlighter(): SyntaxHighlighter = StrykeSyntaxHighlighter()
    override fun getDemoText(): String = DEMO
    override fun getAdditionalHighlightingTagToDescriptorMap(): MutableMap<String, TextAttributesKey>? = null
    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = attrs
    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
    override fun getDisplayName(): String = "Stryke"

    companion object {
        private val DEMO = """
            # stryke demo
            use strict
            fn greet(\$name) {
                p "hello, \$name"
            }
            my @nums = 1:10
            my @doubled = @nums |> map { _ * 2 } |> grep { _ > 5 }
            my %h = (a => 1, b => 2)
            my \$re = "foo bar" =~ /\b(\w+)\b/
            greet("world")
        """.trimIndent()
    }
}
