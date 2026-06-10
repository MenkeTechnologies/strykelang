package com.menketechnologies.stryke.navigate

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.ide.DataManager
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionPlaces
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.ex.ActionUtil
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.platform.lsp.api.LspServer
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.menketechnologies.stryke.StrykeFileType
import com.menketechnologies.stryke.lsp.StrykeLspServerSupportProvider
import org.eclipse.lsp4j.DefinitionParams
import org.eclipse.lsp4j.Location
import org.eclipse.lsp4j.LocationLink
import org.eclipse.lsp4j.Position
import org.eclipse.lsp4j.TextDocumentIdentifier
import org.eclipse.lsp4j.jsonrpc.messages.Either

/**
 * Bridges Cmd-B (`Go To → Declaration or Usages`) on `.stk` files so
 * that pressing it on a declaration shows the usages popup. Without
 * this handler the platform's LSP auto-wiring just reports "Cannot
 * find declaration to go to" because the LSP server returns no target
 * when the cursor IS the declaration.
 *
 * `GotoDeclarationOrUsageHandler2` falls through GTD → ShowUsages
 * only for languages with PSI references. Stryke's flat parser has
 * none, so we drive ShowUsages ourselves when GTD would be empty.
 *
 * Sync invocation via [ActionUtil.invokeAction] (NOT
 * `ApplicationManager.invokeLater`) — the latter caused a race where
 * sometimes the Find Usages tool window opened instead of the popup.
 *
 * Refs:
 *   https://plugins.jetbrains.com/docs/intellij/extension-point-list.html
 *   https://github.com/JetBrains/intellij-community/blob/idea/243.22562.145/platform/platform-api/src/com/intellij/openapi/actionSystem/ex/ActionUtil.kt
 */
class StrykeGotoDeclarationHandler : GotoDeclarationHandler {

    override fun getGotoDeclarationTargets(
        sourceElement: PsiElement?,
        offset: Int,
        editor: Editor?,
    ): Array<PsiElement>? {
        if (editor == null) return null
        val project: Project = editor.project ?: return null
        val virtualFile = FileDocumentManager.getInstance().getFile(editor.document)
            ?: return null
        if (virtualFile.fileType != StrykeFileType) return null

        val server: LspServer = LspServerManager.getInstance(project)
            .getServersForProvider(StrykeLspServerSupportProvider::class.java)
            .firstOrNull() ?: return null

        val doc = editor.document
        val line = doc.getLineNumber(offset)
        val col = offset - doc.getLineStartOffset(line)
        val docUri = server.getDocumentIdentifier(virtualFile).uri
        val params = DefinitionParams(TextDocumentIdentifier(docUri), Position(line, col))
        dbg("caret line=$line col=$col → textDocument/definition")

        val result: Either<List<Location>, List<LocationLink>>? = try {
            server.sendRequestSync(LspServer.DEFAULT_REQUEST_TIMEOUT_MS) { lsp4j ->
                lsp4j.textDocumentService.definition(params)
            }
        } catch (t: Throwable) {
            dbg("EXCEPTION sending definition: ${t::class.java.simpleName}: ${t.message}")
            return null
        }
        val defTargets: List<Pair<String, Position>> = when {
            result == null -> emptyList()
            result.isLeft -> result.left.orEmpty().map { it.uri to it.range.start }
            result.isRight -> result.right.orEmpty().map { it.targetUri to it.targetRange.start }
            else -> emptyList()
        }
        dbg("definition returned ${defTargets.size} target(s)")

        // LSP returns empty when cursor is on a declaration (the
        // server's goto_definition returns None for sym.decl_line ==
        // pos.line). That's our signal to fire ShowUsages directly —
        // the platform won't bridge to it on its own for an LSP-backed
        // language without PSI refs.
        val atDecl = defTargets.isEmpty()
            || defTargets.any { it.first == docUri && it.second.line == line }
        if (atDecl) {
            dbg("cursor at decl → invoking ShowUsages, returning self-target")
            triggerShowUsages(editor, offset)
            // Return the PsiElement at the current cursor offset so
            // the platform's GTD pipeline sees a non-empty target and
            // suppresses the "Cannot find declaration to go to" balloon.
            // Platform's navigate(target) DOES shift the caret to the
            // leaf's textRange.startOffset, which often differs from
            // the exact cursor offset — `triggerShowUsages` restores
            // the original offset on the EDT after dispatch so the
            // cursor visibly stays put.
            return selfTarget(project, virtualFile, offset)
        }
        // Usage→decl: platform's auto-wired LSP definition handler
        // handles it. Returning targets here would compete with that.
        return null
    }

    /**
     * PsiElement at `offset` in the given file, suitable as a "navigate
     * to self" sentinel. Resolved under a read action because PSI
     * traversal isn't EDT-safe (and we're called on a pooled thread).
     */
    private fun selfTarget(
        project: Project,
        virtualFile: com.intellij.openapi.vfs.VirtualFile,
        offset: Int,
    ): Array<PsiElement>? {
        return ReadAction.compute<Array<PsiElement>?, RuntimeException> {
            val psiFile = PsiManager.getInstance(project).findFile(virtualFile) ?: return@compute null
            val leaf: PsiElement = psiFile.findElementAt(offset) ?: psiFile
            arrayOf(leaf)
        }
    }

    /**
     * Invoke IntelliJ's built-in `ShowUsages` action. The platform
     * calls `getGotoDeclarationTargets` on a pooled thread (under a
     * read action), but `DataManager.getDataContext` + the action-
     * system require EDT — schedule the actual dispatch via
     * `invokeLater`. The race with the platform's own GTD pipeline
     * that justified the earlier sync attempt no longer applies: the
     * LSP server now returns None when the cursor is on the decl, so
     * the platform has nothing to navigate to and won't fight us.
     */
    private fun triggerShowUsages(editor: Editor, originalOffset: Int) {
        val action = ActionManager.getInstance().getAction("ShowUsages") ?: run {
            dbg("ABORT: no `ShowUsages` action registered")
            return
        }
        ApplicationManager.getApplication().invokeLater {
            // The user can close the tab / switch files in the gap
            // between `selfTarget` returning on the pooled thread and
            // this EDT runnable firing. Any access to a disposed
            // editor (caretModel, component, ...) throws
            // TraceableDisposable$DisposalException — bail silently.
            if (editor.isDisposed) {
                dbg("ABORT: editor disposed before EDT dispatch")
                return@invokeLater
            }
            // Restore the caret to where the user pressed Cmd-B BEFORE
            // firing ShowUsages. The platform's earlier navigate(self-
            // target) call may have shifted the caret to the leaf's
            // start offset; rewind so the popup anchors at the actual
            // click position AND the cursor stays put when the popup
            // closes.
            if (editor.caretModel.offset != originalOffset) {
                editor.caretModel.moveToOffset(originalOffset)
                dbg("restored caret offset $originalOffset")
            }
            val ctx = DataManager.getInstance().getDataContext(editor.component)
            val event = AnActionEvent.createFromAnAction(
                action,
                null,
                ActionPlaces.UNKNOWN,
                ctx,
            )
            ActionUtil.invokeAction(action, event, null)
            dbg("ShowUsages action dispatched on EDT")
        }
    }

    private fun dbg(msg: String) {
        com.menketechnologies.stryke.StrykeDebugLog.log("gotodef", msg)
    }
}
