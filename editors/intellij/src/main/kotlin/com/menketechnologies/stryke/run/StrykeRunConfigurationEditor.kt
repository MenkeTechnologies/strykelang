package com.menketechnologies.stryke.run

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class StrykeRunConfigurationEditor : SettingsEditor<StrykeRunConfiguration>() {
    private val scriptField = TextFieldWithBrowseButton().apply {
        addBrowseFolderListener(
            "Stryke Script",
            "Choose a .stk file to run",
            null,
            FileChooserDescriptorFactory.createSingleFileNoJarsDescriptor()
                .withFileFilter { it.extension == "stk" },
        )
    }
    private val scriptArgsField = JBTextField()
    private val interpreterArgsField = JBTextField()
    private val workDirField = TextFieldWithBrowseButton().apply {
        addBrowseFolderListener(
            "Working Directory",
            "Choose the run working directory",
            null,
            FileChooserDescriptorFactory.createSingleFolderDescriptor(),
        )
    }
    private val noInteropCheck = JBCheckBox("--no-interop (strict stryke parser)")

    private val panel: JPanel = FormBuilder.createFormBuilder()
        .addLabeledComponent("Script:", scriptField)
        .addLabeledComponent("Script arguments:", scriptArgsField)
        .addLabeledComponent("Interpreter arguments:", interpreterArgsField)
        .addLabeledComponent("Working directory:", workDirField)
        .addComponent(noInteropCheck)
        .addComponentFillVertically(JPanel(), 0)
        .panel

    override fun createEditor(): JComponent = panel

    override fun resetEditorFrom(s: StrykeRunConfiguration) {
        scriptField.text = s.options.scriptPath.orEmpty()
        scriptArgsField.text = s.options.scriptArgs.orEmpty()
        interpreterArgsField.text = s.options.interpreterArgs.orEmpty()
        workDirField.text = s.options.workingDirectory.orEmpty()
        noInteropCheck.isSelected = s.options.noInterop
    }

    override fun applyEditorTo(s: StrykeRunConfiguration) {
        s.options.scriptPath = scriptField.text
        s.options.scriptArgs = scriptArgsField.text
        s.options.interpreterArgs = interpreterArgsField.text
        s.options.workingDirectory = workDirField.text
        s.options.noInterop = noInteropCheck.isSelected
    }
}
