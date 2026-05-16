package com.menketechnologies.stryke.run

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.JBUI
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
    private val disasmCheck = JBCheckBox("--disasm (bytecode disassembly to stderr)")
    private val profileCheck = JBCheckBox("--profile (wall-clock VM-op profile to stderr)")
    private val flameCheck = JBCheckBox("--flame (terminal/svg flamegraph)")
    private val debugFlagCheck = JBCheckBox("-d (Perl-style debugger)")
    private val debugFlagsField = JBTextField()

    private val panel: JPanel = FormBuilder.createFormBuilder()
        .addComponent(header("Program"))
        .addLabeledComponent("Script:", scriptField)
        .addLabeledComponent("Script arguments:", scriptArgsField)
        .addLabeledComponent("Interpreter arguments:", interpreterArgsField)
        .addLabeledComponent("Working directory:", workDirField)

        .addComponent(header("Parser"))
        .addComponent(noInteropCheck)

        .addComponent(header("Tracing / debug"))
        .addComponent(disasmCheck)
        .addComponent(profileCheck)
        .addComponent(flameCheck)
        .addComponent(debugFlagCheck)
        .addLabeledComponent("Debug flags (-D):", debugFlagsField)
        .addTooltip("Letter combinations or numbers, e.g. `tls`, `4`. See `st -D?`.")

        .addComponentFillVertically(JPanel(), 0)
        .panel.apply { border = JBUI.Borders.empty(8) }

    private fun header(title: String) =
        JBLabel("<html><b>$title</b></html>").apply { border = JBUI.Borders.emptyTop(8) }

    override fun createEditor(): JComponent = panel

    override fun resetEditorFrom(s: StrykeRunConfiguration) {
        scriptField.text = s.options.scriptPath.orEmpty()
        scriptArgsField.text = s.options.scriptArgs.orEmpty()
        interpreterArgsField.text = s.options.interpreterArgs.orEmpty()
        workDirField.text = s.options.workingDirectory.orEmpty()
        noInteropCheck.isSelected = s.options.noInterop
        disasmCheck.isSelected = s.options.disasm
        profileCheck.isSelected = s.options.profile
        flameCheck.isSelected = s.options.flame
        debugFlagCheck.isSelected = s.options.debugFlag
        debugFlagsField.text = s.options.debugFlags.orEmpty()
    }

    override fun applyEditorTo(s: StrykeRunConfiguration) {
        s.options.scriptPath = scriptField.text
        s.options.scriptArgs = scriptArgsField.text
        s.options.interpreterArgs = interpreterArgsField.text
        s.options.workingDirectory = workDirField.text
        s.options.noInterop = noInteropCheck.isSelected
        s.options.disasm = disasmCheck.isSelected
        s.options.profile = profileCheck.isSelected
        s.options.flame = flameCheck.isSelected
        s.options.debugFlag = debugFlagCheck.isSelected
        s.options.debugFlags = debugFlagsField.text
    }
}
