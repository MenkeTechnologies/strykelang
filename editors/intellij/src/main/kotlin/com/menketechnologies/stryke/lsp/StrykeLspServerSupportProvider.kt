package com.menketechnologies.stryke.lsp

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.LspServerSupportProvider.LspServerStarter
import com.menketechnologies.stryke.StrykeSettings

class StrykeLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(project: Project, file: VirtualFile, serverStarter: LspServerStarter) {
        val settings = StrykeSettings.getInstance()
        if (!settings.lspEnabled) return
        val ext = file.extension ?: return
        if (ext !in settings.supportedExtensions()) return
        serverStarter.ensureServerStarted(StrykeLspServerDescriptor(project))
    }
}
