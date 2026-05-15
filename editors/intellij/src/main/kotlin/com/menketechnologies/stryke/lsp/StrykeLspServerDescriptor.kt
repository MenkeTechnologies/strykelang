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

    override fun isSupportedFile(file: VirtualFile): Boolean = file.extension == "stk"

    override fun createCommandLine(): GeneralCommandLine {
        val exe = resolveStExe()
        LOG.info("Starting stryke LSP: $exe --lsp")
        return GeneralCommandLine(exe, "--lsp")
            .withWorkDirectory(project.basePath ?: PathManager.getHomePath())
            .withEnvironment("RUST_BACKTRACE", "1")
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

    companion object {
        private val LOG = Logger.getInstance(StrykeLspServerDescriptor::class.java)
    }
}
