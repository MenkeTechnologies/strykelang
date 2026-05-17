package com.menketechnologies.stryke.refactor

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.platform.lsp.api.LspServer
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.platform.lsp.util.applyTextEdits
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.refactoring.rename.RenameHandler
import com.menketechnologies.stryke.StrykeFileType
import com.menketechnologies.stryke.lsp.StrykeLspServerSupportProvider
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.RenameParams
import org.eclipse.lsp4j.TextDocumentIdentifier
import org.eclipse.lsp4j.WorkspaceEdit

/**
 * Handles Shift-F6 (Rename) and Ctrl-T → Rename on `.stk` files by
 * routing through the LSP server's `textDocument/rename` endpoint.
 *
 * IntelliJ's default `RenameHandler` only fires for languages whose PSI
 * names elements (variables, functions, etc.) — our flat parser doesn't
 * do that, so without this handler Shift-F6 silently no-ops. Here we
 * pop a dialog asking for the new name, then ask the LSP to compute
 * the rename edits and apply the returned [WorkspaceEdit].
 */
class StrykeRenameHandler : RenameHandler {

    override fun isAvailableOnDataContext(dataContext: DataContext): Boolean {
        val file = dataContext.getData(CommonDataKeys.PSI_FILE) ?: return false
        return file.fileType == StrykeFileType
    }

    override fun isRenaming(dataContext: DataContext): Boolean = isAvailableOnDataContext(dataContext)

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        if (editor == null || file == null) return
        val virtualFile = file.virtualFile ?: return
        val server = LspServerManager.getInstance(project)
            .getServersForProvider(StrykeLspServerSupportProvider::class.java)
            .firstOrNull() ?: return

        val offset = editor.caretModel.offset
        val doc = editor.document
        val line = doc.getLineNumber(offset)
        val col = offset - doc.getLineStartOffset(line)
        val pos = Position(line, col)

        // Heuristic: pre-fill the dialog with the identifier under the caret.
        val identifier = identifierAt(doc.charsSequence, offset)
        val newName = Messages.showInputDialog(
            project,
            "Rename '${identifier.ifEmpty { "<identifier>" }}' to:",
            "Rename",
            null,
            identifier,
            null,
        ) ?: return
        if (newName.isBlank() || newName == identifier) return

        val params = RenameParams(
            TextDocumentIdentifier(server.getDocumentIdentifier(virtualFile).uri),
            pos,
            newName,
        )
        val edit: WorkspaceEdit? = server.sendRequestSync(
            LspServer.DEFAULT_REQUEST_TIMEOUT_MS,
        ) { lsp4j -> lsp4j.textDocumentService.rename(params) }

        if (edit == null) return

        // Apply edits manually: for each URI in the WorkspaceEdit's
        // `changes` map, find the document and run `applyTextEdits`. The
        // platform's `Lsp4jUtilKt.applyTextEdits` handles document-version
        // checks + reverse-sorted offset application for us.
        val changes = edit.changes ?: emptyMap()
        WriteCommandAction.runWriteCommandAction(project) {
            for ((uri, edits) in changes) {
                val vf = VirtualFileManager.getInstance().findFileByUrl(uri) ?: continue
                val document = FileDocumentManager.getInstance().getDocument(vf) ?: continue
                applyTextEdits(document, edits)
            }
        }
        FileDocumentManager.getInstance().saveAllDocuments()
    }

    override fun invoke(project: Project, elements: Array<PsiElement>, dataContext: DataContext?) {
        // Element-array form unused for our use case.
    }

    private fun identifierAt(chars: CharSequence, offset: Int): String {
        if (offset < 0 || offset > chars.length) return ""
        var s = offset
        var e = offset
        while (s > 0 && isIdentChar(chars[s - 1])) s--
        while (e < chars.length && isIdentChar(chars[e])) e++
        if (s == e) return ""
        return chars.subSequence(s, e).toString()
    }

    private fun isIdentChar(c: Char): Boolean = c == '_' || c.isLetterOrDigit()
}
