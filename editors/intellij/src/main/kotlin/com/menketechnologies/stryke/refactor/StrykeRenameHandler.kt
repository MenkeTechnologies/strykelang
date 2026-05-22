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
        dbg("invoked")
        if (editor == null) { dbg("ABORT: no editor"); return }
        if (file == null) { dbg("ABORT: no file"); return }
        val virtualFile = file.virtualFile ?: run { dbg("ABORT: no virtualFile"); return }
        val server = LspServerManager.getInstance(project)
            .getServersForProvider(StrykeLspServerSupportProvider::class.java)
            .firstOrNull()
        if (server == null) { dbg("ABORT: no LSP server"); return }
        dbg("server=${server.descriptor.presentableName} state=${server.state}")

        val offset = editor.caretModel.offset
        val doc = editor.document
        val line = doc.getLineNumber(offset)
        val col = offset - doc.getLineStartOffset(line)
        val pos = Position(line, col)
        val identifier = identifierAt(doc.charsSequence, offset)
        dbg("caret line=$line col=$col offset=$offset identifier='$identifier'")

        val newName = Messages.showInputDialog(
            project,
            "Rename '${identifier.ifEmpty { "<identifier>" }}' to:",
            "Rename",
            null,
            identifier,
            null,
        )
        if (newName == null) { dbg("ABORT: user cancelled"); return }
        if (newName.isBlank()) { dbg("ABORT: blank newName"); return }
        if (newName == identifier) { dbg("ABORT: unchanged"); return }
        dbg("newName='$newName'")

        val params = RenameParams(
            TextDocumentIdentifier(server.getDocumentIdentifier(virtualFile).uri),
            pos,
            newName,
        )
        dbg("sending textDocument/rename uri=${params.textDocument.uri}")
        val edit: WorkspaceEdit? = try {
            server.sendRequestSync(LspServer.DEFAULT_REQUEST_TIMEOUT_MS) { lsp4j ->
                lsp4j.textDocumentService.rename(params)
            }
        } catch (t: Throwable) {
            dbg("EXCEPTION sending rename: ${t::class.java.simpleName}: ${t.message}")
            Messages.showErrorDialog(project, "LSP rename request failed: ${t.message}", "Rename")
            return
        }
        if (edit == null) {
            dbg("ABORT: LSP returned null WorkspaceEdit — server refused the rename for this identifier")
            Messages.showWarningDialog(
                project,
                "Cannot rename '$identifier' at this position. The cursor may be on " +
                    "punctuation, a keyword, or an identifier the server can't resolve " +
                    "(common causes: parse error elsewhere in the file, or the symbol is " +
                    "declared in a file that isn't open or `require`d from the current file).",
                "Rename",
            )
            return
        }
        dbg("got WorkspaceEdit changes=${edit.changes?.size ?: 0} documentChanges=${edit.documentChanges?.size ?: 0}")

        // WorkspaceEdit per the LSP spec has TWO mutually-exclusive forms:
        //   - `changes`: Map<URI, TextEdit[]>            (simple form)
        //   - `documentChanges`: TextDocumentEdit[]      (versioned form)
        // Servers MAY emit either. Handle both — the prior code only
        // checked `changes` and dropped servers that use `documentChanges`.
        //
        // `WriteCommandAction.runWriteCommandAction` provides both the
        // CommandProcessor and WriteAction contexts required by
        // `applyTextEdits` (IntentionAction.invoke Javadoc applies here
        // too — document mutations need both wrappers).
        WriteCommandAction.runWriteCommandAction(project) {
            var totalEdits = 0
            edit.changes?.forEach { (uri, edits) ->
                val vf = VirtualFileManager.getInstance().findFileByUrl(uri)
                if (vf == null) { dbg("no VirtualFile for uri=$uri"); return@forEach }
                val document = FileDocumentManager.getInstance().getDocument(vf)
                if (document == null) { dbg("no Document for $uri"); return@forEach }
                dbg("applying ${edits.size} edits to $uri")
                applyTextEdits(document, edits)
                totalEdits += edits.size
            }
            edit.documentChanges?.forEach { dc ->
                if (dc.isLeft) {
                    val tde = dc.left ?: return@forEach
                    val uri = tde.textDocument.uri
                    val vf = VirtualFileManager.getInstance().findFileByUrl(uri)
                    if (vf == null) { dbg("no VirtualFile for uri=$uri (docChanges)"); return@forEach }
                    val document = FileDocumentManager.getInstance().getDocument(vf)
                    if (document == null) { dbg("no Document for $uri (docChanges)"); return@forEach }
                    dbg("applying ${tde.edits.size} edits to $uri (docChanges)")
                    applyTextEdits(document, tde.edits)
                    totalEdits += tde.edits.size
                } else {
                    dbg("skipping non-edit documentChange: ${dc.right?.javaClass?.simpleName}")
                }
            }
            dbg("totalEdits applied = $totalEdits")
        }
        FileDocumentManager.getInstance().saveAllDocuments()
        dbg("done")
    }

    private fun dbg(msg: String) {
        com.menketechnologies.stryke.StrykeDebugLog.log("rename", msg)
    }

    override fun invoke(project: Project, elements: Array<PsiElement>, dataContext: DataContext?) {
        // Element-array form unused for our use case.
    }

    /**
     * Identifier span at `offset`, INCLUDING `::` package separators.
     * `Foo::bar` is one identifier; `Foo::Baz::qux` is one identifier. A
     * single `:` is not part of an identifier — only paired `::`. Without
     * the namespace tail the rename dialog prefills with `bar`, sends
     * `bar` to the LSP, and the LSP can't resolve a sub declaration for
     * the unqualified name.
     */
    private fun identifierAt(chars: CharSequence, offset: Int): String {
        if (offset < 0 || offset > chars.length) return ""
        var s = offset
        var e = offset
        // Walk left
        while (s > 0) {
            val c = chars[s - 1]
            if (isIdentChar(c)) { s--; continue }
            // `::` between two letter/digit/underscore runs
            if (c == ':' && s >= 2 && chars[s - 2] == ':' &&
                s >= 3 && isIdentChar(chars[s - 3])
            ) {
                s -= 2
                continue
            }
            break
        }
        // Walk right
        while (e < chars.length) {
            val c = chars[e]
            if (isIdentChar(c)) { e++; continue }
            if (c == ':' && e + 1 < chars.length && chars[e + 1] == ':' &&
                e + 2 < chars.length && isIdentChar(chars[e + 2])
            ) {
                e += 2
                continue
            }
            break
        }
        if (s == e) return ""
        return chars.subSequence(s, e).toString()
    }

    private fun isIdentChar(c: Char): Boolean = c == '_' || c.isLetterOrDigit()
}
