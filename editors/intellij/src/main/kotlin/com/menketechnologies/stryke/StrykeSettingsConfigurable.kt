package com.menketechnologies.stryke

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

class StrykeSettingsConfigurable : Configurable {
    private val executableField = TextFieldWithBrowseButton().apply {
        addBrowseFolderListener(
            "Stryke Executable",
            "Path to the stryke (st) binary",
            null,
            FileChooserDescriptorFactory.createSingleFileNoJarsDescriptor(),
        )
    }

    private var panel: JPanel? = null

    override fun getDisplayName(): String = "Stryke"

    override fun createComponent(): JComponent {
        val p = FormBuilder.createFormBuilder()
            .addLabeledComponent(JBLabel("Stryke executable:"), executableField, 1, false)
            .addComponentFillVertically(JPanel(), 0)
            .panel
        panel = p
        reset()
        return p
    }

    override fun isModified(): Boolean {
        val saved = StrykeSettings.getInstance().stExecutable ?: ""
        return executableField.text != saved
    }

    override fun apply() {
        StrykeSettings.getInstance().stExecutable = executableField.text.takeIf { it.isNotBlank() }
    }

    override fun reset() {
        executableField.text = StrykeSettings.getInstance().stExecutable ?: ""
    }

    override fun disposeUIResources() { panel = null }
}
