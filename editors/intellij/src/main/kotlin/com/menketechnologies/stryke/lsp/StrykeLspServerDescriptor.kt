package com.menketechnologies.stryke.lsp

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.application.PathManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.SystemInfo
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.menketechnologies.stryke.StrykeSettings
import java.io.File

class StrykeLspServerDescriptor(project: Project) :
    ProjectWideLspServerDescriptor(project, "Stryke") {

    override fun isSupportedFile(file: VirtualFile): Boolean {
        val ext = file.extension ?: return false
        return ext in StrykeSettings.getInstance().supportedExtensions()
    }

    override fun createCommandLine(): GeneralCommandLine {
        val settings = StrykeSettings.getInstance()
        val exe = resolveStExe()
        LOG.info("Starting stryke LSP: $exe --lsp ${settings.extraLspArgs}")
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
