package com.menketechnologies.stryke.actions

import com.intellij.ide.actions.CreateFileFromTemplateAction
import com.intellij.ide.actions.CreateFileFromTemplateDialog
import com.intellij.ide.actions.WeighingActionGroup
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDirectory
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiFileFactory
import com.menketechnologies.stryke.StrykeFileType
import com.menketechnologies.stryke.StrykeIcons

/// File > New > Stryke File. Hands the user a name dialog with a few
/// canonical starting templates (script, library, empty). All
/// templates resolve to `StrykeFileType` so the new buffer
/// immediately picks up syntax highlighting, LSP, etc.
///
/// Implemented via the platform's `CreateFileFromTemplateAction` so
/// we inherit the standard New-File dialog (name field, template
/// picker, undoable PSI write). Templates are inline string literals
/// here rather than `fileTemplates/internal/*.stk` so the plugin
/// stays single-jar with no resource extraction at runtime.
///
/// **Weighing**: the platform's `WeighingNewActionGroup` wraps
/// `NewGroup` and runs `WeighingActionGroup.postProcessVisibleChildren`
/// to promote only the heaviest-weighted children to the top of File
/// > New / right-click → New, dumping everything else into a buried
/// "Other" sub-popup. Default action weight is 0; the only built-ins
/// that `shouldBeChosenAnyway` whitelists at top level are
/// `CreateFileAction` / `CreateDirectoryOrPackageAction` /
/// `NewModuleInGroupAction`. Without an explicit weight bump our
/// action gets demoted into "Other" and the user concludes it doesn't
/// exist. The Groovy plugin (`NewGroovyClassAction.update`) solves
/// this by setting `WEIGHT_KEY = HIGHER_WEIGHT` once it confirms a
/// Groovy-source context — we do the equivalent unconditionally on
/// visibility so the action surfaces every time the parent's
/// `isAvailable` says we have a writable directory.
class CreateStrykeFileAction :
    CreateFileFromTemplateAction("Stryke File", "Create new stryke script", StrykeIcons.FILE),
    DumbAware {

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        super.update(e)
        val presentation = e.presentation
        if (presentation.isVisible) {
            // Surface at top level of the File > New / right-click → New
            // popup instead of being demoted into the platform's "Other"
            // sub-group. See class kdoc for why this is required.
            presentation.putClientProperty(
                WeighingActionGroup.WEIGHT_KEY,
                WeighingActionGroup.HIGHER_WEIGHT,
            )
        }
    }

    override fun getActionName(directory: PsiDirectory?, newName: String, templateName: String?): String =
        "Create Stryke File"

    override fun buildDialog(
        project: Project,
        directory: PsiDirectory,
        builder: CreateFileFromTemplateDialog.Builder,
    ) {
        builder
            .setTitle("New Stryke File")
            .addKind("Script (#!/usr/bin/env stryke)", StrykeIcons.FILE, TPL_SCRIPT)
            .addKind("Library / module",                StrykeIcons.FILE, TPL_LIB)
            .addKind("Empty",                           StrykeIcons.FILE, TPL_EMPTY)
    }

    override fun createFile(name: String, templateName: String, dir: PsiDirectory): PsiFile? {
        val fileName = if (name.contains('.')) name else "$name.stk"
        val body = when (templateName) {
            TPL_SCRIPT -> SCRIPT_BODY
            TPL_LIB    -> LIB_BODY
            else       -> ""
        }
        val file = PsiFileFactory.getInstance(dir.project)
            .createFileFromText(fileName, StrykeFileType, body)
        return dir.add(file) as? PsiFile
    }

    companion object {
        private const val TPL_SCRIPT = "Script"
        private const val TPL_LIB    = "Library"
        private const val TPL_EMPTY  = "Empty"

        private val SCRIPT_BODY = """
            |#!/usr/bin/env stryke
            |# vim:ft=stryke
            |
            |fn main {
            |    p "hello from stryke"
            |}
            |
            |main
            |""".trimMargin()

        private val LIB_BODY = """
            |# Stryke module — import this from another script.
            |
            |fn greet(name) {
            |    "hello, ${'$'}{name}"
            |}
            |""".trimMargin()
    }
}
