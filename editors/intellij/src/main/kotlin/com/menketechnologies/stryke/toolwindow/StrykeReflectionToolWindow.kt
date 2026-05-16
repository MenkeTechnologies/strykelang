package com.menketechnologies.stryke.toolwindow

import com.google.gson.JsonElement
import com.google.gson.JsonParser
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.SimpleToolWindowPanel
import com.intellij.openapi.util.SystemInfo
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.SearchTextField
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.components.JBTabbedPane
import com.intellij.ui.treeStructure.Tree
import com.intellij.util.ui.JBUI
import com.menketechnologies.stryke.StrykeSettings
import java.awt.BorderLayout
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.io.File
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.SwingUtilities
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel
import javax.swing.tree.TreePath

class StrykeReflectionToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = StrykeReflectionPanel(project)
        val content = toolWindow.contentManager.factory.createContent(panel, "", false)
        toolWindow.contentManager.addContent(content)
    }

    override fun shouldBeAvailable(project: Project): Boolean = true
}

private class StrykeReflectionPanel(private val project: Project) : SimpleToolWindowPanel(true, true) {

    private val tabs = JBTabbedPane()
    private val statusLabel = JBLabel(" ")

    /** Each tab keeps a handle so refresh / search can be re-applied without rebuilding. */
    private val tabsByName = LinkedHashMap<String, HashTabPanel>()

    init {
        layout = BorderLayout()
        // Toolbar
        val group = DefaultActionGroup().apply {
            add(RefreshAction())
            addSeparator()
            add(OpenSettingsAction())
        }
        val actionToolbar = com.intellij.openapi.actionSystem.ActionManager.getInstance()
            .createActionToolbar("StrykeReflectionToolbar", group, true)
        actionToolbar.targetComponent = this
        toolbar = actionToolbar.component

        // Body
        val body = JPanel(BorderLayout())
        body.add(tabs, BorderLayout.CENTER)
        body.add(statusLabel.apply { border = JBUI.Borders.empty(4, 8) }, BorderLayout.SOUTH)
        setContent(body)

        // Load asynchronously
        reload()
    }

    private fun reload() {
        statusLabel.text = "Loading reflection data from `st`…"
        tabs.removeAll()
        tabsByName.clear()
        ApplicationManager.getApplication().executeOnPooledThread {
            val (data, err) = loadReflection()
            SwingUtilities.invokeLater {
                if (data == null) {
                    statusLabel.text = "Failed to load: ${err.orEmpty()}"
                    return@invokeLater
                }
                for ((name, json) in data.entrySet()) {
                    val tab = HashTabPanel(project, name, json)
                    tabsByName[name] = tab
                    tabs.addTab(tabTitle(name, tab.entryCount), tab)
                }
                statusLabel.text = "${data.entrySet().sumOf { it.value.asJsonObject.size() }} entries across ${data.size()} hashes"
            }
        }
    }

    private fun tabTitle(name: String, count: Int): String =
        "${prettyHashName(name)}  ${count}"

    private fun loadReflection(): Pair<com.google.gson.JsonObject?, String?> {
        val exe = resolveSt() ?: return null to "stryke executable not found (Settings → Tools → Stryke)"
        val script = """
            my %dump = (
              all          => \%stryke::all,
              builtins     => \%stryke::builtins,
              keywords     => \%stryke::keywords,
              operators    => \%stryke::operators,
              special_vars => \%stryke::special_vars,
              perl_compats => \%stryke::perl_compats,
              extensions   => \%stryke::extensions,
              aliases      => \%stryke::aliases,
              descriptions => \%stryke::descriptions,
            );
            p tj(\%dump);
        """.trimIndent()
        return try {
            val cmd = GeneralCommandLine(exe, "-e", script).withCharset(Charsets.UTF_8)
            val handler = CapturingProcessHandler(cmd)
            val out = handler.runProcess(15_000)
            if (out.exitCode != 0) return null to "exit ${out.exitCode}: ${out.stderr.take(400)}"
            val json = JsonParser.parseString(out.stdout).asJsonObject
            json to null
        } catch (e: Exception) {
            LOG.warn("reflection dump failed", e)
            null to (e.message ?: e.javaClass.simpleName)
        }
    }

    private fun resolveSt(): String? {
        val cfg = StrykeSettings.getInstance().stExecutable
        if (!cfg.isNullOrBlank() && File(cfg).canExecute()) return cfg
        val pathEnv = System.getenv("PATH") ?: return null
        val suffixes = if (SystemInfo.isWindows) listOf(".exe", ".bat", ".cmd", "") else listOf("")
        for (dir in pathEnv.split(File.pathSeparator)) {
            for (suf in suffixes) {
                for (name in listOf("st", "stryke")) {
                    val f = File(dir, name + suf)
                    if (f.canExecute()) return f.absolutePath
                }
            }
        }
        return null
    }

    private fun prettyHashName(name: String): String = when (name) {
        "all" -> "All %all"
        "builtins" -> "Builtins %b"
        "keywords" -> "Keywords %k"
        "operators" -> "Operators %o"
        "special_vars" -> "Special vars %v"
        "perl_compats" -> "Perl5 %pc"
        "extensions" -> "Extensions %e"
        "aliases" -> "Aliases %a"
        "descriptions" -> "Descriptions %d"
        else -> name
    }

    private inner class RefreshAction : AnAction("Refresh", "Re-run `st` and reload reflection data", AllIcons.Actions.Refresh) {
        override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
        override fun actionPerformed(e: AnActionEvent) = reload()
    }

    private inner class OpenSettingsAction : AnAction("Settings", "Open Stryke settings", AllIcons.General.Settings) {
        override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
        override fun actionPerformed(e: AnActionEvent) {
            com.intellij.openapi.options.ShowSettingsUtil.getInstance()
                .showSettingsDialog(project, "Stryke")
        }
    }

    companion object {
        private val LOG = Logger.getInstance(StrykeReflectionPanel::class.java)
    }
}

private class HashTabPanel(
    private val project: Project,
    private val hashName: String,
    private val data: JsonElement,
) : JPanel(BorderLayout()) {

    private val root = DefaultMutableTreeNode(hashName)
    private val model = DefaultTreeModel(root)
    private val tree = Tree(model).apply {
        isRootVisible = false
        showsRootHandles = true
    }
    private val search = SearchTextField()

    val entryCount: Int

    init {
        val obj = data.asJsonObject
        entryCount = obj.size()

        // Group by category (the value) so the tree mirrors stryke's own grouping.
        val byCategory: MutableMap<String, MutableList<Pair<String, String>>> = sortedMapOf()
        for ((k, v) in obj.entrySet()) {
            val cat = if (v.isJsonPrimitive) v.asString else v.toString()
            byCategory.getOrPut(cat) { mutableListOf() }.add(k to cat)
        }
        for ((cat, entries) in byCategory) {
            val catNode = DefaultMutableTreeNode("$cat  (${entries.size})")
            for ((name, _) in entries.sortedBy { it.first }) {
                catNode.add(DefaultMutableTreeNode(name))
            }
            root.add(catNode)
        }
        model.reload()

        add(buildHeader(), BorderLayout.NORTH)
        add(JBScrollPane(tree), BorderLayout.CENTER)

        tree.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                if (e.clickCount != 1
                    || !javax.swing.SwingUtilities.isLeftMouseButton(e)
                    || e.isPopupTrigger
                ) return
                // Only fire docs popup for leaf clicks. Category nodes are
                // expanded/collapsed by the tree's default click handler.
                val path = tree.getPathForLocation(e.x, e.y) ?: return
                val node = path.lastPathComponent as? DefaultMutableTreeNode ?: return
                if (!node.isLeaf) return
                val name = (node.userObject as? String) ?: return
                showDocsPopupAt(name, e)
            }
        })

        // Right-click context menu. IntelliJ's PopupHandler handles both
        // platform-correct trigger detection (right-click on macOS = Ctrl-click)
        // and action-group integration.
        com.intellij.ui.PopupHandler.installPopupHandler(
            tree,
            buildContextActionGroup(),
            "StrykeReflectionPopup",
        )

        search.addDocumentListener(object : javax.swing.event.DocumentListener {
            override fun insertUpdate(e: javax.swing.event.DocumentEvent) = applyFilter()
            override fun removeUpdate(e: javax.swing.event.DocumentEvent) = applyFilter()
            override fun changedUpdate(e: javax.swing.event.DocumentEvent) = applyFilter()
        })
    }

    private fun buildHeader(): JComponent {
        val p = JPanel(BorderLayout())
        p.border = JBUI.Borders.empty(4)
        p.add(search, BorderLayout.CENTER)
        return p
    }

    private fun applyFilter() {
        val q = search.text.trim().lowercase()
        // Rebuild root by filtering.
        root.removeAllChildren()
        val obj = data.asJsonObject
        val byCategory: MutableMap<String, MutableList<String>> = sortedMapOf()
        for ((k, v) in obj.entrySet()) {
            val cat = if (v.isJsonPrimitive) v.asString else v.toString()
            val matches = q.isEmpty() || k.lowercase().contains(q) || cat.lowercase().contains(q)
            if (matches) byCategory.getOrPut(cat) { mutableListOf() }.add(k)
        }
        for ((cat, entries) in byCategory) {
            val catNode = DefaultMutableTreeNode("$cat  (${entries.size})")
            for (name in entries.sorted()) catNode.add(DefaultMutableTreeNode(name))
            root.add(catNode)
        }
        model.reload()
        if (q.isNotEmpty()) expandAll()
    }

    private fun expandAll() {
        var i = 0
        while (i < tree.rowCount) { tree.expandRow(i); i++ }
    }

    /**
     * Variant of [showDocsPopup] that anchors at the mouse-click point rather
     * than the keyboard-derived data context (used for menu invocations).
     */
    private fun showDocsPopupAt(name: String, e: MouseEvent) {
        com.intellij.openapi.application.ApplicationManager.getApplication().executeOnPooledThread {
            val rawText = fetchDocsRaw(name)
            com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater {
                val console = com.intellij.execution.filters.TextConsoleBuilderFactory
                    .getInstance()
                    .createBuilder(project)
                    .console as com.intellij.execution.ui.ConsoleView
                val decoder = com.intellij.execution.process.AnsiEscapeDecoder()
                decoder.escapeText(
                    rawText,
                    com.intellij.execution.process.ProcessOutputTypes.STDOUT,
                ) { fragment, outputType ->
                    val ct = com.intellij.execution.ui.ConsoleViewContentType.getConsoleViewType(outputType)
                    console.print(fragment, ct)
                }
                val component = console.component
                component.preferredSize = java.awt.Dimension(720, 420)
                val popup = com.intellij.openapi.ui.popup.JBPopupFactory.getInstance()
                    .createComponentPopupBuilder(component, console.preferredFocusableComponent)
                    .setTitle("stryke docs: $name")
                    .setResizable(true)
                    .setMovable(true)
                    .setRequestFocus(true)
                    .setCancelOnClickOutside(true)
                    .createPopup()
                com.intellij.openapi.util.Disposer.register(popup) { console.dispose() }
                popup.show(com.intellij.ui.awt.RelativePoint(e))
            }
        }
    }

    /** Name of the leaf under the current selection, or null if a category. */
    private fun selectedLeafName(): String? {
        val sel = tree.selectionPath ?: return null
        val node = sel.lastPathComponent as? DefaultMutableTreeNode ?: return null
        if (!node.isLeaf) return null
        return node.userObject as? String
    }

    private fun buildContextActionGroup(): com.intellij.openapi.actionSystem.ActionGroup {
        val group = com.intellij.openapi.actionSystem.DefaultActionGroup()
        group.add(ShowDocsAction())
        group.add(CopyNameAction())
        return group
    }

    private inner class ShowDocsAction : com.intellij.openapi.actionSystem.AnAction(
        "Show Docs", "Open the hover doc card for this name", com.intellij.icons.AllIcons.Actions.Help,
    ) {
        override fun getActionUpdateThread() = com.intellij.openapi.actionSystem.ActionUpdateThread.BGT
        override fun update(e: com.intellij.openapi.actionSystem.AnActionEvent) {
            e.presentation.isEnabled = selectedLeafName() != null
        }
        override fun actionPerformed(e: com.intellij.openapi.actionSystem.AnActionEvent) {
            val name = selectedLeafName() ?: return
            showDocsPopup(name, e)
        }
    }

    private inner class CopyNameAction : com.intellij.openapi.actionSystem.AnAction(
        "Copy Name", "Copy this name to the clipboard", com.intellij.icons.AllIcons.Actions.Copy,
    ) {
        override fun getActionUpdateThread() = com.intellij.openapi.actionSystem.ActionUpdateThread.BGT
        override fun update(e: com.intellij.openapi.actionSystem.AnActionEvent) {
            e.presentation.isEnabled = selectedLeafName() != null
        }
        override fun actionPerformed(e: com.intellij.openapi.actionSystem.AnActionEvent) {
            val name = selectedLeafName() ?: return
            val sel = java.awt.datatransfer.StringSelection(name)
            java.awt.Toolkit.getDefaultToolkit().systemClipboard.setContents(sel, sel)
        }
    }

    private fun showDocsPopup(name: String, e: com.intellij.openapi.actionSystem.AnActionEvent) {
        com.intellij.openapi.application.ApplicationManager.getApplication().executeOnPooledThread {
            // Keep raw ANSI — we'll let IntelliJ's Console render it with colors.
            val rawText = fetchDocsRaw(name)
            com.intellij.openapi.application.ApplicationManager.getApplication().invokeLater {
                val console = com.intellij.execution.filters.TextConsoleBuilderFactory
                    .getInstance()
                    .createBuilder(project)
                    .console as com.intellij.execution.ui.ConsoleView

                // Walk the ANSI escapes and emit each segment with the
                // matching ConsoleViewContentType (foreground / bg / style).
                val decoder = object : com.intellij.execution.process.AnsiEscapeDecoder() {
                    public override fun getCurrentOutputAttributes(
                        outputType: com.intellij.openapi.util.Key<*>,
                    ): com.intellij.openapi.util.Key<*> = super.getCurrentOutputAttributes(outputType)
                }
                decoder.escapeText(
                    rawText,
                    com.intellij.execution.process.ProcessOutputTypes.STDOUT,
                ) { fragment, outputType ->
                    val ct = com.intellij.execution.ui.ConsoleViewContentType
                        .getConsoleViewType(outputType)
                    console.print(fragment, ct)
                }

                val component: javax.swing.JComponent = console.component
                component.preferredSize = java.awt.Dimension(720, 420)

                val popup = com.intellij.openapi.ui.popup.JBPopupFactory.getInstance()
                    .createComponentPopupBuilder(component, console.preferredFocusableComponent)
                    .setTitle("stryke docs: $name")
                    .setResizable(true)
                    .setMovable(true)
                    .setRequestFocus(true)
                    .setCancelOnClickOutside(true)
                    .setCancelOnOtherWindowOpen(true)
                    .createPopup()
                com.intellij.openapi.util.Disposer.register(popup) { console.dispose() }
                if (e.inputEvent != null) popup.showInBestPositionFor(e.dataContext)
                else popup.showCenteredInCurrentWindow(project)
            }
        }
    }

    /** ANSI-stripped version (kept for non-console callers). */
    private fun fetchDocs(name: String): String =
        stripAnsi(fetchDocsRaw(name)).trim().ifBlank { "(no docs for $name)" }

    /** Raw `stryke docs <name>` output, ANSI escapes intact. */
    private fun fetchDocsRaw(name: String): String {
        val exe = resolveSt() ?: return "stryke executable not found — set it in Settings → Tools → Stryke"
        return try {
            val cmd = com.intellij.execution.configurations.GeneralCommandLine(exe, "docs", name)
                .withCharset(Charsets.UTF_8)
                .withEnvironment("FORCE_COLOR", "1")
                .withEnvironment("CLICOLOR_FORCE", "1")
            val handler = com.intellij.execution.process.CapturingProcessHandler(cmd)
            val out = handler.runProcess(8_000)
            val raw = if (out.exitCode == 0) out.stdout else out.stderr.ifBlank { out.stdout }
            raw.trim().ifBlank { "(no docs for $name)" }
        } catch (e: Exception) {
            "Failed to fetch docs: ${e.message}"
        }
    }

    private fun resolveSt(): String? {
        val cfg = com.menketechnologies.stryke.StrykeSettings.getInstance().stExecutable
        if (!cfg.isNullOrBlank() && java.io.File(cfg).canExecute()) return cfg
        val pathEnv = System.getenv("PATH") ?: return null
        val suffixes = if (com.intellij.openapi.util.SystemInfo.isWindows) listOf(".exe", ".bat", ".cmd", "") else listOf("")
        for (dir in pathEnv.split(java.io.File.pathSeparator)) {
            for (suf in suffixes) {
                for (n in listOf("st", "stryke")) {
                    val f = java.io.File(dir, n + suf)
                    if (f.canExecute()) return f.absolutePath
                }
            }
        }
        return null
    }

    private fun stripAnsi(s: String): String =
        s.replace(Regex("\\[[0-9;?]*[A-Za-z]"), "")
}
