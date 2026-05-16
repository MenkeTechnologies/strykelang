package com.menketechnologies.stryke.dap

import com.google.gson.JsonObject
import com.intellij.openapi.application.ApplicationManager
import com.intellij.xdebugger.XSourcePosition
import com.intellij.xdebugger.evaluation.XDebuggerEvaluator
import com.intellij.xdebugger.frame.XValuePlace

/**
 * Evaluator backing the *Evaluate Expression* dialog and inline hovers.
 *
 * Sends the expression through DAP `evaluate`. The server resolves it from
 * the current snapshot (just locals + globals for v1 — no full expression
 * parsing). When the frame is gone (post-resume), the IDE shows the
 * familiar "stack frame doesn't support evaluation" message because
 * `XStackFrame.getEvaluator` returns null in that state.
 */
class StrykeEvaluator(
    private val client: StrykeDapClient?,
    private val frameId: Int,
) : XDebuggerEvaluator() {

    override fun evaluate(
        expression: String,
        callback: XEvaluationCallback,
        expressionPosition: XSourcePosition?,
    ) {
        val c = client
        if (c == null || !c.isAlive()) {
            callback.errorOccurred("Debugger not connected")
            return
        }
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val args = JsonObject().apply {
                    addProperty("expression", expression)
                    addProperty("frameId", frameId)
                    addProperty("context", "watch")
                }
                val body = c.request("evaluate", args)
                if (body == null) {
                    callback.errorOccurred("Evaluation timed out")
                    return@executeOnPooledThread
                }
                val result = body.get("result")?.asString ?: ""
                callback.evaluated(StrykeValue(name = expression, repr = result, kind = "scalar"))
            } catch (e: Exception) {
                callback.errorOccurred(e.message ?: "Evaluation failed")
            }
        }
    }
}
