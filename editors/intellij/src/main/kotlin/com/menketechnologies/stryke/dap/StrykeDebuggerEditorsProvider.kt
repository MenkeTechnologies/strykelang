package com.menketechnologies.stryke.dap

import com.intellij.lang.Language
import com.intellij.openapi.fileTypes.FileType
import com.intellij.openapi.project.Project
import com.intellij.xdebugger.XExpression
import com.intellij.xdebugger.XSourcePosition
import com.intellij.xdebugger.evaluation.EvaluationMode
import com.intellij.xdebugger.evaluation.XDebuggerEditorsProvider
import com.menketechnologies.stryke.StrykeFileType
import com.menketechnologies.stryke.StrykeLanguage
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.impl.LoadTextUtil
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiFileFactory
import com.intellij.psi.PsiDocumentManager

class StrykeDebuggerEditorsProvider : XDebuggerEditorsProvider() {
    override fun getFileType(): FileType = StrykeFileType

    override fun createDocument(
        project: Project,
        expression: XExpression,
        sourcePosition: XSourcePosition?,
        mode: EvaluationMode,
    ): Document {
        val psi = PsiFileFactory.getInstance(project).createFileFromText(
            "_stryke_expr.stk",
            StrykeFileType,
            expression.expression,
        )
        return PsiDocumentManager.getInstance(project).getDocument(psi)
            ?: com.intellij.openapi.editor.EditorFactory.getInstance().createDocument(expression.expression)
    }
}
