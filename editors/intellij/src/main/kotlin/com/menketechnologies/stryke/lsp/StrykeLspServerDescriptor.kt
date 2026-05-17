package com.menketechnologies.stryke.lsp

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.application.PathManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.SystemInfo
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspSemanticTokensSupport
import com.menketechnologies.stryke.StrykeSettings
import java.io.File

class StrykeLspServerDescriptor(project: Project) :
    ProjectWideLspServerDescriptor(project, "Stryke") {

    override fun isSupportedFile(file: VirtualFile): Boolean {
        val ext = file.extension ?: return false
        return ext in StrykeSettings.getInstance().supportedExtensions()
    }

    // ── LSP customization (2026.1 API) ────────────────────────────────────
    // In 2026.1 the LSP API uses `lspCustomization: LspCustomization` with
    // customizer **fields** on the LspCustomization object. The previous
    // deprecated `lspSemanticTokensSupport` / `lspCodeActionsSupport` etc.
    // direct properties on LspServerDescriptor are IGNORED in 261.x.
    //
    // Reference (verbatim 261 Prisma plugin):
    // https://github.com/JetBrains/intellij-plugins/blob/idea/261.23567.138/prisma/src/org/intellij/prisma/ide/lsp/PrismaLspServerDescriptor.kt
    //
    // All LSP features are enabled by default in 2026.1. We only override
    // `semanticTokensCustomizer` to provide the LSP-type → IntelliJ-color
    // mapping so `$pass` inside `"..."` colors as a variable rather than
    // falling back to string-green.
    override val lspCustomization: LspCustomization = object : LspCustomization() {
        override val semanticTokensCustomizer: LspSemanticTokensSupport = object : LspSemanticTokensSupport() {
            override fun getTextAttributesKey(
                tokenType: String,
                tokenModifiers: List<String>,
            ): com.intellij.openapi.editor.colors.TextAttributesKey? {
                val key: com.intellij.openapi.editor.colors.TextAttributesKey? = when (tokenType) {
                    "keyword" -> com.menketechnologies.stryke.StrykeColors.KEYWORD
                    "function" -> com.menketechnologies.stryke.StrykeColors.FUNCTION_CALL
                    "variable" -> com.menketechnologies.stryke.StrykeColors.SCALAR_VAR
                    "parameter" -> com.menketechnologies.stryke.StrykeColors.PARAMETER
                    "string" -> com.menketechnologies.stryke.StrykeColors.STRING
                    "number" -> com.menketechnologies.stryke.StrykeColors.NUMBER
                    "comment" -> com.menketechnologies.stryke.StrykeColors.COMMENT
                    "operator" -> com.menketechnologies.stryke.StrykeColors.OPERATOR
                    "regexp" -> com.menketechnologies.stryke.StrykeColors.REGEX
                    "macro" -> com.menketechnologies.stryke.StrykeColors.FUNCTION_CALL
                    "type" -> com.menketechnologies.stryke.StrykeColors.PACKAGE_NAME
                    "class" -> com.menketechnologies.stryke.StrykeColors.PACKAGE_NAME
                    "property" -> com.menketechnologies.stryke.StrykeColors.HASH_VAR
                    "namespace" -> com.menketechnologies.stryke.StrykeColors.PACKAGE_NAME
                    else -> null
                }
                val keyName = key?.externalName ?: "null"
                LOG.info("getTextAttributesKey(type=$tokenType, mods=$tokenModifiers) → $keyName")
                com.menketechnologies.stryke.StrykeDebugLog.log(
                    "semtoken",
                    "type=$tokenType mods=$tokenModifiers → $keyName",
                )
                return key
            }
        }
    }

    override fun createCommandLine(): GeneralCommandLine {
        val settings = StrykeSettings.getInstance()
        val exe = resolveStExe()
        LOG.info("Starting stryke LSP: $exe --lsp ${settings.extraLspArgs}")
        com.menketechnologies.stryke.StrykeDebugLog.log(
            "lsp",
            "createCommandLine exe=$exe args=--lsp ${settings.extraLspArgs} cwd=${project.basePath}",
        )
        val cmd = GeneralCommandLine(exe)
            .withParameters("--lsp")
            .withWorkDirectory(project.basePath ?: PathManager.getHomePath())
            .withEnvironment("RUST_BACKTRACE", "1")
        // Extra LSP args from settings
        splitArgs(settings.extraLspArgs).forEach { cmd.addParameter(it) }
        // Optional KEY=VAL env from settings
        for (kv in splitArgs(settings.lspEnv)) {
            val i = kv.indexOf('=')
            if (i > 0) cmd.withEnvironment(kv.substring(0, i), kv.substring(i + 1))
        }
        if (settings.logLspToFile && settings.lspLogPath.isNotBlank()) {
            cmd.withEnvironment("STRYKE_LSP_LOG", settings.lspLogPath)
            com.menketechnologies.stryke.StrykeDebugLog.log(
                "lsp",
                "STRYKE_LSP_LOG=${settings.lspLogPath}",
            )
        }
        return cmd
    }

    private fun resolveStExe(): String {
        val settings = StrykeSettings.getInstance()
        settings.stExecutable
            ?.takeIf { it.isNotBlank() && File(it).canExecute() }
            ?.let { return it }
        return findOnPath("st") ?: findOnPath("stryke") ?: "st"
    }

    private fun findOnPath(name: String): String? {
        val pathEnv = System.getenv("PATH") ?: return null
        val sep = File.pathSeparator
        val suffixes = if (SystemInfo.isWindows) listOf(".exe", ".bat", ".cmd", "") else listOf("")
        for (dir in pathEnv.split(sep)) {
            for (suf in suffixes) {
                val f = File(dir, name + suf)
                if (f.canExecute()) return f.absolutePath
            }
        }
        return null
    }

    private fun splitArgs(s: String): List<String> {
        if (s.isBlank()) return emptyList()
        val out = mutableListOf<String>()
        val sb = StringBuilder()
        var quote: Char? = null
        for (c in s) {
            when {
                quote != null && c == quote -> quote = null
                quote != null -> sb.append(c)
                c == '"' || c == '\'' -> quote = c
                c.isWhitespace() -> if (sb.isNotEmpty()) { out += sb.toString(); sb.clear() }
                else -> sb.append(c)
            }
        }
        if (sb.isNotEmpty()) out += sb.toString()
        return out
    }

    companion object {
        private val LOG = Logger.getInstance(StrykeLspServerDescriptor::class.java)
    }
}
