package com.menketechnologies.stryke.run

import com.intellij.execution.ExecutionException
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.configurations.RunProfile
import com.intellij.execution.configurations.RunProfileState
import com.intellij.execution.executors.DefaultDebugExecutor
import com.intellij.execution.process.KillableColoredProcessHandler
import com.intellij.execution.runners.ExecutionEnvironment
import com.intellij.execution.runners.DefaultProgramRunner
import com.intellij.execution.ui.RunContentDescriptor
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.util.io.FileUtil
import com.intellij.xdebugger.XDebugProcess
import com.intellij.xdebugger.XDebugProcessStarter
import com.intellij.xdebugger.XDebugSession
import com.intellij.xdebugger.XDebuggerManager
import com.menketechnologies.stryke.StrykeSettings
import com.menketechnologies.stryke.dap.StrykeDebugProcess

/**
 * Debug executor handler for [StrykeRunConfiguration].
 *
 * Synchronous `doExecute` path — the standard pattern used by every JetBrains
 * debug runner (Java, Kotlin, Python, Go). We spawn `st --dap`, hand the
 * resulting `ProcessHandler` to a fresh `StrykeDebugProcess`, register it
 * with `XDebuggerManager.startSession`, and return the session's content
 * descriptor.
 *
 * `session.runContentDescriptor` triggers a deprecation log in split-mode
 * IDEs (RustRover 2024.3+), but there's no replacement in the public API yet
 * — the warning is cosmetic. JetBrains' own debug runners have not migrated.
 */
class StrykeDebugRunner : DefaultProgramRunner() {
    override fun getRunnerId(): String = "StrykeDebugRunner"

    override fun canRun(executorId: String, profile: RunProfile): Boolean =
        executorId == DefaultDebugExecutor.EXECUTOR_ID && profile is StrykeRunConfiguration

    @Throws(ExecutionException::class)
    override fun doExecute(state: RunProfileState, env: ExecutionEnvironment): RunContentDescriptor? {
        val cfg = env.runProfile as StrykeRunConfiguration
        val exe = StrykeSettings.getInstance().stExecutable?.takeIf { it.isNotBlank() } ?: "st"

        // Open a ServerSocket FIRST so we know the port the spawned `st --dap`
        // should connect back to. This sidesteps the stdout-sharing race that
        // would otherwise corrupt DAP traffic — `OSProcessHandler` reads the
        // process stdout for the Console, and `DapClient` reads its own socket
        // exclusively.
        val serverSocket = java.net.ServerSocket()
        serverSocket.reuseAddress = true
        serverSocket.bind(java.net.InetSocketAddress("127.0.0.1", 0))
        val port = serverSocket.localPort

        val cmd = GeneralCommandLine()
            .withExePath(exe)
            .withCharset(Charsets.UTF_8)
            .withParameters("--dap", "127.0.0.1:$port")
        val wd = cfg.options.workingDirectory?.takeIf { it.isNotBlank() }
            ?: FileUtil.toSystemDependentName(env.project.basePath ?: ".")
        cmd.withWorkDirectory(wd)
        // KillableColoredProcessHandler renders ANSI escapes from stryke
        // (test runner, `p` color output) as real colors in the Debug Console.
        val handler = KillableColoredProcessHandler(cmd)

        // Block briefly waiting for stryke to connect back. 10s ceiling.
        serverSocket.soTimeout = 10_000
        val dapSocket: java.net.Socket = try {
            serverSocket.accept()
        } catch (e: java.net.SocketTimeoutException) {
            handler.destroyProcess()
            throw com.intellij.execution.ExecutionException(
                "Stryke debug: `st --dap 127.0.0.1:$port` didn't connect within 10s. " +
                    "Is the `st` binary fresh? (Settings → Tools → Stryke executable)",
            )
        }
        serverSocket.close()

        val session: XDebugSession = XDebuggerManager.getInstance(env.project).startSession(
            env,
            object : XDebugProcessStarter() {
                override fun start(session: XDebugSession): XDebugProcess {
                    val args = splitArgs(cfg.options.scriptArgs.orEmpty())
                    return StrykeDebugProcess(
                        session = session,
                        processHandler = handler,
                        dapSocket = dapSocket,
                        programPath = cfg.options.scriptPath.orEmpty(),
                        programArgs = args,
                        workingDirectory = wd,
                    )
                }
            },
        )

        // RustRover 2026.1 `XDebugSession.runContentDescriptor` getter fires
        // `Logger.error("[Split debugger] ...")` and pops a red "Internal IDE
        // Error" toast — the deprecation is the platform telling plugins to
        // migrate to `XDebuggerManagerProxy` (still `internal`). But
        // `XDebugSessionImpl` also exposes `getMockRunContentDescriptorIfInitialized()`
        // which returns the SAME descriptor without the noisy log call.
        // Invoke it via reflection so we don't take a compile-time dependency
        // on the platform's internal API surface.
        return getDescriptorWithoutSplitDebuggerWarning(session)
            // Fallback if JetBrains renames the method in a future release.
            ?: @Suppress("DEPRECATION") session.runContentDescriptor
    }

    /**
     * Invoke `XDebugSessionImpl#getMockRunContentDescriptorIfInitialized()`
     * by reflection. Returns null when the method isn't present or returns
     * null (descriptor not yet built).
     */
    private fun getDescriptorWithoutSplitDebuggerWarning(session: XDebugSession): RunContentDescriptor? {
        return try {
            val m = session.javaClass.methods.firstOrNull {
                it.name == "getMockRunContentDescriptorIfInitialized" && it.parameterCount == 0
            } ?: return null
            m.isAccessible = true
            m.invoke(session) as? RunContentDescriptor
        } catch (e: Throwable) {
            LOG.debug("getMockRunContentDescriptorIfInitialized reflection failed", e)
            null
        }
    }

    private fun splitArgs(s: String): List<String> {
        if (s.isBlank()) return emptyList()
        val out = mutableListOf<String>()
        val sb = StringBuilder()
        var quote: Char? = null
        for (c in s) {
            when {
                quote != null && c == quote -> quote = null
                quote != null -> sb.append(c)
                c == '"' || c == '\'' -> quote = c
                c.isWhitespace() -> if (sb.isNotEmpty()) { out += sb.toString(); sb.clear() }
                else -> sb.append(c)
            }
        }
        if (sb.isNotEmpty()) out += sb.toString()
        return out
    }

    companion object {
        private val LOG = Logger.getInstance(StrykeDebugRunner::class.java)
    }
}
