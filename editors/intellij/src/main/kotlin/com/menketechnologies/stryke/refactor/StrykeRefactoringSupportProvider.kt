package com.menketechnologies.stryke.refactor

import com.intellij.lang.refactoring.RefactoringSupportProvider
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.CommandProcessor
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.SelectionModel
import com.intellij.openapi.project.Project
import com.intellij.platform.lsp.api.LspServer
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.platform.lsp.api.customization.LspIntentionAction
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.refactoring.RefactoringActionHandler
import com.menketechnologies.stryke.lsp.StrykeLspServerSupportProvider
import org.eclipse.lsp4j.CodeAction
import org.eclipse.lsp4j.CodeActionContext
import org.eclipse.lsp4j.CodeActionParams
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.Range
import org.eclipse.lsp4j.TextDocumentIdentifier
import org.eclipse.lsp4j.jsonrpc.messages.Either

/**
 * Bridges IntelliJ's keymap-driven refactoring actions (Cmd-Opt-M /
 * Cmd-Opt-V / Cmd-Opt-C / Ctrl-T "Refactor This") into the LSP code
 * actions returned by `stryke --lsp`.
 *
 * IntelliJ's LSP integration only surfaces code actions via Alt-Enter
 * intentions. The dedicated refactoring keymaps go through a separate
 * `RefactoringSupportProvider`/`RefactoringActionHandler` path that
 * has no built-in LSP awareness. This provider implements that bridge:
 * the handler sends `textDocument/codeAction` with the current
 * selection, filters the response by title heuristic, and runs the
 * resulting [LspIntentionAction].
 *
 * The title heuristic matches the strings emitted by
 * `strykelang/lsp_extras.rs::compute_code_actions`. If those titles
 * change, update the [titleMatches] predicates below.
 */
class StrykeRefactoringSupportProvider : RefactoringSupportProvider() {
    override fun isAvailable(context: PsiElement): Boolean = true
    override fun isMemberInplaceRenameAvailable(element: PsiElement, context: PsiElement?): Boolean = true
    override fun isInplaceRenameAvailable(element: PsiElement, context: PsiElement?): Boolean = true

    override fun getExtractMethodHandler(): RefactoringActionHandler =
        LspExtractActionHandler(
            "Extract Method",
            { it.contains("function") || it.contains("method") },
            hint = "Method extraction requires a multi-line selection. Use Cmd-Opt-V or Cmd-Opt-C for single-line expressions.",
        )

    override fun getIntroduceVariableHandler(): RefactoringActionHandler =
        LspExtractActionHandler("Extract Variable", { it.contains("variable") && !it.contains("constant") })

    override fun getIntroduceConstantHandler(): RefactoringActionHandler =
        LspExtractActionHandler("Extract Constant", { it.contains("constant") })
}

/**
 * Generic handler that asks the LSP for code actions covering the
 * current selection, then runs the first one whose title matches
 * [titleMatches]. If nothing matches, the user sees a no-op (status-bar
 * notice would be nicer but the IntelliJ refactoring path expects
 * silent failure when no refactoring applies).
 */
private class LspExtractActionHandler(
    private val refactoringName: String,
    private val titleMatches: (String) -> Boolean,
    private val hint: String? = null,
) : RefactoringActionHandler {

    override fun invoke(
        project: Project,
        editor: Editor?,
        file: PsiFile?,
        dataContext: DataContext?,
    ) {
        dbg("invoked")
        LOG.info("LspExtractActionHandler($refactoringName) invoked")
        if (editor == null) {
            dbg("ABORT: no editor")
            notifyUser(project, "$refactoringName: no editor")
            return
        }
        if (file == null) {
            dbg("ABORT: no file")
            notifyUser(project, "$refactoringName: no file")
            return
        }
        val virtualFile = file.virtualFile
        if (virtualFile == null) {
            dbg("ABORT: file has no VirtualFile")
            notifyUser(project, "$refactoringName: file has no VirtualFile")
            return
        }
        val server = findStrykeLspServer(project)
        if (server == null) {
            dbg("ABORT: no LSP server found via LspServerManager.getServersForProvider(StrykeLspServerSupportProvider::class.java)")
            notifyUser(project, "$refactoringName: LSP server not running. Check Help → Show Log for `Starting stryke LSP:`.")
            return
        }
        dbg("file=${virtualFile.path} server=${server.descriptor.presentableName} state=${server.state}")

        val selection = editor.selectionModel
        val (range, hasSelection) = selectionRange(editor.document, selection)
        dbg("selection: hasSelection=$hasSelection range=$range startOffset=${selection.selectionStart} endOffset=${selection.selectionEnd} text=${selection.selectedText?.take(80)?.replace('\n', '⏎')}")
        if (!hasSelection) {
            notifyUser(project, "$refactoringName: select an expression first, then invoke this action.")
            return
        }

        LOG.info("LspExtractActionHandler($refactoringName) sending textDocument/codeAction for range $range")
        val params = CodeActionParams(
            TextDocumentIdentifier(server.getDocumentIdentifier(virtualFile).uri),
            range,
            CodeActionContext(emptyList()),
        )

        dbg("sending textDocument/codeAction…")
        val response: List<Either<org.eclipse.lsp4j.Command, CodeAction>>? = try {
            server.sendRequestSync(
                LspServer.DEFAULT_REQUEST_TIMEOUT_MS,
            ) { lsp4j -> lsp4j.textDocumentService.codeAction(params) }
        } catch (t: Throwable) {
            dbg("EXCEPTION sending codeAction: ${t::class.java.simpleName}: ${t.message}")
            notifyUser(project, "$refactoringName: LSP request threw ${t::class.java.simpleName}: ${t.message}")
            return
        }

        LOG.info("LspExtractActionHandler($refactoringName) got ${response?.size ?: 0} actions")
        dbg("got ${response?.size ?: 0} actions")
        if (response.isNullOrEmpty()) {
            notifyUser(project, "$refactoringName: LSP returned no code actions for this range.")
            return
        }
        val candidates = response.mapNotNull { e -> if (e.isRight) e.right else null }
        dbg("candidate titles: ${candidates.joinToString(" | ") { it.title }}")
        val match: CodeAction = candidates.firstOrNull { titleMatches(it.title.lowercase()) }
            ?: run {
                val titles = candidates.joinToString("; ") { it.title }
                val tail = if (hint != null) "  $hint" else ""
                dbg("ABORT: no title matched filter; got: $titles")
                notifyUser(project, "$refactoringName: no matching action. LSP returned: $titles$tail")
                return
            }

        LOG.info("LspExtractActionHandler($refactoringName) applying '${match.title}'")
        dbg("applying '${match.title}'")
        // Per `IntentionAction.invoke` Javadoc:
        //   "This method is called inside a command (see CommandProcessor).
        //    If startInWriteAction() returns true, this method is also
        //    called inside a write action."
        //
        // Two wrappers are required when invoking an IntentionAction
        // programmatically from a non-Alt-Enter context:
        //   1. CommandProcessor.executeCommand — provides the undo/redo
        //      grouping and document-mutation context.
        //   2. WriteCommandAction — required if `startInWriteAction()` is
        //      true. `LspIntentionAction` returns false (it manages its
        //      own WriteAction internally via WriteAction.run inside
        //      invoke), so only CommandProcessor is needed at this layer.
        //
        // Plus: `isAvailable` MUST be called first to prime the wrapper's
        // internal `uriToDocumentMap` — without that priming, `invoke` is
        // a silent no-op (see the LspIntentionAction.class disassembly).
        try {
            val intention = LspIntentionAction(server, match)
            val available = intention.isAvailable(project, editor, file)
            dbg("isAvailable() = $available  startInWriteAction=${intention.startInWriteAction()}")
            if (!available) {
                notifyUser(project, "$refactoringName: LSP intention reported isAvailable=false; no edit applied.")
                return
            }
            CommandProcessor.getInstance().executeCommand(
                project,
                {
                    try {
                        intention.invoke(project, editor, file)
                        dbg("intention.invoke() returned cleanly inside CommandProcessor")
                    } catch (t: Throwable) {
                        dbg("EXCEPTION inside command: ${t::class.java.simpleName}: ${t.message}")
                        throw t
                    }
                },
                "Stryke: $refactoringName",
                "stryke.refactor",
            )
        } catch (t: Throwable) {
            dbg("EXCEPTION applying intention: ${t::class.java.simpleName}: ${t.message}")
            notifyUser(project, "$refactoringName: applying the WorkspaceEdit threw ${t::class.java.simpleName}: ${t.message}")
        }
    }

    private fun dbg(msg: String) {
        com.menketechnologies.stryke.StrykeDebugLog.log("refactor:$refactoringName", msg)
    }

    override fun invoke(
        project: Project,
        elements: Array<PsiElement>,
        dataContext: DataContext?,
    ) {
        // Element-array form isn't used by Cmd-Opt-M / Cmd-T paths in
        // practice — they always pass through the editor variant above.
    }

    private fun selectionRange(
        document: com.intellij.openapi.editor.Document,
        selection: SelectionModel,
    ): Pair<Range, Boolean> {
        val startOffset = selection.selectionStart
        val endOffset = selection.selectionEnd
        val hasSelection = startOffset != endOffset
        val startLine = document.getLineNumber(startOffset)
        val endLine = document.getLineNumber(endOffset)
        val startCol = startOffset - document.getLineStartOffset(startLine)
        val endCol = endOffset - document.getLineStartOffset(endLine)
        return Range(
            Position(startLine, startCol),
            Position(endLine, endCol),
        ) to hasSelection
    }

    private fun findStrykeLspServer(project: Project): LspServer? =
        LspServerManager.getInstance(project)
            .getServersForProvider(StrykeLspServerSupportProvider::class.java)
            .firstOrNull()

    private fun notifyUser(project: Project, message: String) {
        LOG.warn(message)
        // Use a balloon notification so failures are visible without
        // having to dig through idea.log.
        val group = NotificationGroupManager.getInstance()
            .getNotificationGroup("Stryke Refactoring")
            ?: NotificationGroupManager.getInstance().getNotificationGroup("Other")
        group?.createNotification(message, NotificationType.WARNING)
            ?.notify(project)
    }

    companion object {
        private val LOG = Logger.getInstance(LspExtractActionHandler::class.java)
    }
}
