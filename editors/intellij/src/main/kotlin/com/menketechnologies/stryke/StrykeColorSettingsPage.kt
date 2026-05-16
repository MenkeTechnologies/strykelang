package com.menketechnologies.stryke

import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import javax.swing.Icon

class StrykeColorSettingsPage : ColorSettingsPage {
    private val attrs = arrayOf(
        // Comments & strings
        AttributesDescriptor("Comments//Line comment", StrykeColors.COMMENT),
        AttributesDescriptor("Comments//Doc comment (##, #!)", StrykeColors.DOC_COMMENT),
        AttributesDescriptor("Strings//String", StrykeColors.STRING),
        AttributesDescriptor("Strings//Heredoc", StrykeColors.HEREDOC),
        AttributesDescriptor("Strings//String escape", StrykeColors.STRING_ESCAPE),

        // Numbers
        AttributesDescriptor("Numbers//Integer", StrykeColors.NUMBER),
        AttributesDescriptor("Numbers//Float", StrykeColors.FLOAT),

        // Regex
        AttributesDescriptor("Regex//Pattern", StrykeColors.REGEX),
        AttributesDescriptor("Regex//Flags", StrykeColors.REGEX_FLAGS),

        // Keywords
        AttributesDescriptor("Keywords//Keyword (generic)", StrykeColors.KEYWORD),
        AttributesDescriptor("Keywords//Declaration (my, our, local, state, use)", StrykeColors.DECL_KEYWORD),
        AttributesDescriptor("Keywords//Function/class (fn, sub, class, struct, trait)", StrykeColors.FN_KEYWORD),
        AttributesDescriptor("Keywords//Control flow (if, while, for, return)", StrykeColors.CONTROL_KEYWORD),
        AttributesDescriptor("Keywords//Phase (BEGIN, END, INIT, CHECK)", StrykeColors.PHASE_KEYWORD),
        AttributesDescriptor("Keywords//Word operator (and, or, eq, cmp, x)", StrykeColors.WORD_OPERATOR),
        AttributesDescriptor("Keywords//Boolean (true, false)", StrykeColors.BOOLEAN),
        AttributesDescriptor("Keywords//undef", StrykeColors.UNDEF),

        // Names
        AttributesDescriptor("Names//Builtin (p, map, join)", StrykeColors.BUILTIN),
        AttributesDescriptor("Names//Parallel builtin (pmap, pgrep, spawn)", StrykeColors.BUILTIN_PARALLEL),
        AttributesDescriptor("Names//Function call", StrykeColors.FUNCTION_CALL),
        AttributesDescriptor("Names//Function declaration", StrykeColors.FUNCTION_DECL),
        AttributesDescriptor("Names//Identifier", StrykeColors.IDENTIFIER),
        AttributesDescriptor("Names//Package name (Foo::Bar)", StrykeColors.PACKAGE_NAME),
        AttributesDescriptor("Names//Package separator (::)", StrykeColors.PACKAGE_SEPARATOR),
        AttributesDescriptor("Names//Label", StrykeColors.LABEL),

        // Variables
        AttributesDescriptor("Variables//Sigil ($ @ % bare)", StrykeColors.SIGIL),
        AttributesDescriptor("Variables//Scalar variable (\$name)", StrykeColors.SCALAR_VAR),
        AttributesDescriptor("Variables//Array variable (@name)", StrykeColors.ARRAY_VAR),
        AttributesDescriptor("Variables//Hash variable (%name)", StrykeColors.HASH_VAR),
        AttributesDescriptor("Variables//Special variable (\$!, \$@, \$/)", StrykeColors.SPECIAL_VAR),
        AttributesDescriptor("Variables//Topic (\$_, @_, _)", StrykeColors.TOPIC_VAR),
        AttributesDescriptor("Variables//Block parameter (_0, _1, _N)", StrykeColors.BLOCK_PARAM),
        AttributesDescriptor("Variables//Parameter", StrykeColors.PARAMETER),

        // Operators
        AttributesDescriptor("Operators//Generic operator", StrykeColors.OPERATOR),
        AttributesDescriptor("Operators//Assignment (=, +=, -=)", StrykeColors.ASSIGN_OP),
        AttributesDescriptor("Operators//Arrow (->)", StrykeColors.ARROW_OP),
        AttributesDescriptor("Operators//Fat comma (=>)", StrykeColors.FAT_COMMA),
        AttributesDescriptor("Operators//Pipe (|>, ~>, |>>)", StrykeColors.PIPE),
        AttributesDescriptor("Operators//Range (..)", StrykeColors.RANGE),
        AttributesDescriptor("Operators//Regex bind (=~, !~)", StrykeColors.REGEX_BIND),

        // Punctuation
        AttributesDescriptor("Punctuation//Parentheses ( )", StrykeColors.PAREN),
        AttributesDescriptor("Punctuation//Braces { }", StrykeColors.BRACE),
        AttributesDescriptor("Punctuation//Brackets [ ]", StrykeColors.BRACKET),
        AttributesDescriptor("Punctuation//Comma", StrykeColors.COMMA),
        AttributesDescriptor("Punctuation//Semicolon", StrykeColors.SEMICOLON),
        AttributesDescriptor("Punctuation//Dot", StrykeColors.DOT),

        // Errors
        AttributesDescriptor("Errors//Bad character", StrykeColors.BAD_CHAR),
    )

    override fun getIcon(): Icon = StrykeIcons.FILE
    override fun getHighlighter(): SyntaxHighlighter = StrykeSyntaxHighlighter()
    override fun getDemoText(): String = DEMO
    override fun getAdditionalHighlightingTagToDescriptorMap(): MutableMap<String, TextAttributesKey>? = null
    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = attrs
    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
    override fun getDisplayName(): String = "Stryke"

    companion object {
        private const val D = "$"
        private val DEMO = """
            #!/usr/bin/env st
            # Stryke demo — every token category for color tweaking.
            use strict
            package Demo::Anagram

            ## doc-style comment, used for module documentation
            fn canonical(${D}word) {
                my ${D}t0    = now_ns()
                my @chars   = split //, lc(${D}word)
                my @sorted  = sort { _0 cmp _1 } @chars
                my ${D}r     = join("", @sorted)
                td_add(${D}td_lat, (now_ns() - ${D}t0) / 1000.0)
                return ${D}r
            }

            my @data    = 1:1_000_000
            my @doubled = @data |> pmap { _ * 2 } |> grep { _ > 5 }
            my %h       = (host => "localhost", port => 5432, ttl => 3.14)

            BEGIN { p "boot pid=${D}${D}" }

            if (${D}_ =~ /^(\w+):(\d+)$/i) {
                p "matched ${D}1 on port ${D}2"
            } elsif (defined ${D}!) {
                warn "errno: ${D}!"
            } else {
                p undef
            }
        """.trimIndent()
    }
}
