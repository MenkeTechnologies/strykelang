package com.menketechnologies.stryke.dap

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.intellij.execution.process.ProcessHandler
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.xdebugger.XDebugProcess
import com.intellij.xdebugger.XDebugSession
import com.intellij.xdebugger.breakpoints.XBreakpointHandler
import com.intellij.xdebugger.evaluation.XDebuggerEditorsProvider

/**
 * Bridges IntelliJ XDebugger ↔ stryke `--dap` server.
 *
 * The IDE-side rendering chain is fully synchronous off the DAP `stopped`
 * event: we fetch frames + scopes + variables ONCE, attach them to a
 * pre-built [StrykeStackFrame], call `session.positionReached` with a context
 * that already knows everything. `XStackFrame.computeChildren` just hands
 * back the pre-fetched list — no further DAP round-trip from the UI thread.
 *
 * This pattern avoids the empty-variables-panel symptom that resulted from
 * IntelliJ's split-debugger architecture dropping async expansions.
 */
class StrykeDebugProcess(
    session: XDebugSession,
    private val processHandler: ProcessHandler,
    private val dapSocket: java.net.Socket,
    private val programPath: String,
    private val programArgs: List<String>,
    private val workingDirectory: String?,
) : XDebugProcess(session) {

    @Volatile var client: StrykeDapClient? = null
        private set

    private val executionStack = StrykeExecutionStack()
    private val editorsProvider = StrykeDebuggerEditorsProvider()
    private val breakpointHandlers = arrayOf<XBreakpointHandler<*>>(StrykeBreakpointHandler(this))

    override fun getEditorsProvider(): XDebuggerEditorsProvider = editorsProvider
    override fun getBreakpointHandlers(): Array<XBreakpointHandler<*>> = breakpointHandlers
    override fun doGetProcessHandler(): ProcessHandler = processHandler

    /**
     * Build the Console for the Debug tab and attach it to our process
     * handler so the program's stdout/stderr appears in real time. Without
     * an explicit `createConsole()` override the XDebugProcess base class
     * returns a placeholder that's not wired to our handler, which is why
     * `p "..."` writes go to fd 1 (OSProcessHandler reads them) but the
     * Debug Console stays empty.
     */
    override fun createConsole(): com.intellij.execution.ui.ExecutionConsole {
        val console = com.intellij.execution.filters.TextConsoleBuilderFactory
            .getInstance()
            .createBuilder(session.project)
            .console as com.intellij.execution.ui.ConsoleView
        console.attachToProcess(processHandler)
        return console
    }

    override fun sessionInitialized() {
        super.sessionInitialized()
        // Defer `startNotify` until after the IDE has finished wiring the
        // Console listener onto our process handler. Calling it inline here
        // fires the handler's reader threads BEFORE the listener attaches, so
        // every byte the program emits is dropped → Console looks empty.
        // `invokeLater` punts to the next EDT cycle by which time the
        // RunContentBuilder has registered its listeners.
        ApplicationManager.getApplication().invokeLater {
            if (!processHandler.isStartNotified) {
                processHandler.startNotify()
            }
        }

        // DAP traffic flows over the dedicated socket — NOT the process stdio.
        // Stdio belongs to OSProcessHandler for the program's console output.
        val out = dapSocket.getOutputStream()
        val inp = dapSocket.getInputStream()

        val c = StrykeDapClient(
            output = out,
            input = inp,
            onEvent = { ev, body -> handleEvent(ev, body) },
            onLog = { /* uncomment for trace: LOG.info("DAP: $it") */ },
        )
        client = c

        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                c.request(
                    "initialize",
                    JsonObject().apply {
                        addProperty("clientID", "intellij-stryke")
                        addProperty("clientName", "IntelliJ Stryke")
                        addProperty("adapterID", "stryke")
                        addProperty("locale", "en-US")
                        addProperty("linesStartAt1", true)
                        addProperty("columnsStartAt1", true)
                        addProperty("pathFormat", "path")
                        addProperty("supportsVariableType", true)
                        addProperty("supportsRunInTerminalRequest", false)
                        addProperty("supportsProgressReporting", false)
                    },
                )
                sendAllBreakpoints()
                c.request("configurationDone")
                val launchArgs = JsonObject().apply {
                    addProperty("program", programPath)
                    addProperty("stopOnEntry", false)
                    val args = JsonArray()
                    programArgs.forEach { args.add(it) }
                    add("args", args)
                    workingDirectory?.let { addProperty("cwd", it) }
                }
                c.request("launch", launchArgs)
            } catch (t: Throwable) {
                LOG.warn("DAP init sequence failed", t)
            }
        }
    }

    private fun sendAllBreakpoints() {
        val byFile = mutableMapOf<String, MutableList<Int>>()
        val mgr = com.intellij.xdebugger.XDebuggerManager.getInstance(session.project).breakpointManager
        for (bp in mgr.getBreakpoints(StrykeBreakpointType::class.java)) {
            if (!bp.isEnabled) continue
            val path = bp.fileUrl.removePrefix("file://")
            byFile.getOrPut(path) { mutableListOf() }.add(bp.line + 1)
        }
        val c = client ?: return
        for ((path, lines) in byFile) {
            val args = JsonObject().apply {
                add("source", JsonObject().apply { addProperty("path", path) })
                val arr = JsonArray()
                for (l in lines) {
                    arr.add(JsonObject().apply { addProperty("line", l) })
                }
                add("breakpoints", arr)
            }
            c.requestAsync("setBreakpoints", args)
        }
    }

    private fun handleEvent(event: String, body: JsonObject) {
        when (event) {
            "stopped" -> onStopped(body)
            "terminated" -> session.stop()
            "exited" -> session.stop()
            "output" -> {
                // DAP "output" events should mirror to the Console so the user
                // sees diagnostics + any non-stdout output. In TCP mode the
                // program's real stdout still flows through the processHandler;
                // these events are supplemental.
                val text = body.get("output")?.asString ?: return
                val category = body.get("category")?.asString ?: "stdout"
                val outputType = when (category) {
                    "stderr" -> com.intellij.execution.process.ProcessOutputTypes.STDERR
                    "console" -> com.intellij.execution.process.ProcessOutputTypes.SYSTEM
                    else -> com.intellij.execution.process.ProcessOutputTypes.STDOUT
                }
                processHandler.notifyTextAvailable(text, outputType)
            }
            else -> { /* informational events: initialized, process, thread, etc. */ }
        }
    }

    private fun onStopped(body: JsonObject) {
        // Fetch frames + scopes + variables synchronously, build all
        // StrykeStackFrame objects with their children pre-populated, then
        // hand them to the IDE in one shot.
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val c = client ?: return@executeOnPooledThread

                // 1. stackTrace
                val stArgs = JsonObject().apply {
                    addProperty("threadId", 1)
                    addProperty("startFrame", 0)
                    addProperty("levels", 100)
                }
                val stBody = c.request("stackTrace", stArgs) ?: return@executeOnPooledThread
                val rawFrames = stBody.getAsJsonArray("stackFrames") ?: return@executeOnPooledThread
                if (rawFrames.size() == 0) return@executeOnPooledThread

                val builtFrames = mutableListOf<StrykeStackFrame>()
                for (rf in rawFrames) {
                    val fo = rf.asJsonObject
                    val frameId = fo.get("id")?.asInt ?: 0
                    val frameName = fo.get("name")?.asString ?: "<frame>"
                    val frameFile = fo.getAsJsonObject("source")?.get("path")?.asString ?: ""
                    val frameLine = fo.get("line")?.asInt ?: 0

                    // 2. scopes for this frame
                    val scopesArgs = JsonObject().apply { addProperty("frameId", frameId) }
                    val scopesBody = c.request("scopes", scopesArgs)
                    val scopes = scopesBody?.getAsJsonArray("scopes")

                    // 3. variables for each scope, flattened
                    val children = mutableListOf<StrykeValue>()
                    if (scopes != null) {
                        for (s in scopes) {
                            val so = s.asJsonObject
                            val varRef = so.get("variablesReference")?.asInt ?: continue
                            if (varRef == 0) continue
                            val varsArgs = JsonObject().apply { addProperty("variablesReference", varRef) }
                            val varsBody = c.request("variables", varsArgs) ?: continue
                            val vars = varsBody.getAsJsonArray("variables") ?: continue
                            for (v in vars) {
                                val vo = v.asJsonObject
                                children += StrykeValue(
                                    name = vo.get("name")?.asString ?: "?",
                                    repr = vo.get("value")?.asString ?: "",
                                    kind = vo.get("type")?.asString ?: "scalar",
                                    varRef = vo.get("variablesReference")?.asInt ?: 0,
                                    client = c,
                                )
                            }
                        }
                    }
                    builtFrames += StrykeStackFrame(
                        client = c,
                        frameId = frameId,
                        name = frameName,
                        file = frameFile,
                        line = frameLine,
                        children = children,
                    )
                }

                executionStack.setFrames(builtFrames)
                val ctx = StrykeSuspendContext(executionStack)
                ApplicationManager.getApplication().invokeLater {
                    session.positionReached(ctx)
                }
            } catch (t: Throwable) {
                LOG.warn("onStopped fetch failed", t)
            }
        }
    }

    override fun resume(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("continue", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startStepOver(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("next", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startStepInto(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("stepIn", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startStepOut(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("stepOut", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startPausing() {
        client?.requestAsync("pause", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun stop() {
        client?.requestAsync("disconnect", JsonObject().apply { addProperty("terminateDebuggee", true) })
        client?.close()
        try { dapSocket.close() } catch (_: Exception) {}
    }

    override fun runToPosition(position: com.intellij.xdebugger.XSourcePosition, context: com.intellij.xdebugger.frame.XSuspendContext?) {
        val c = client ?: return
        val path = position.file.path
        val line = position.line + 1
        val args = JsonObject().apply {
            add("source", JsonObject().apply { addProperty("path", path) })
            val arr = JsonArray()
            arr.add(JsonObject().apply { addProperty("line", line) })
            add("breakpoints", arr)
        }
        c.requestAsync("setBreakpoints", args)
        c.requestAsync("continue", JsonObject().apply { addProperty("threadId", 1) })
    }

    companion object {
        private val LOG = Logger.getInstance(StrykeDebugProcess::class.java)
    }
}
