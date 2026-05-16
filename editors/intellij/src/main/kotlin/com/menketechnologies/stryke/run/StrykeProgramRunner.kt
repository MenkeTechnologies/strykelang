package com.menketechnologies.stryke.run

import com.intellij.execution.configurations.RunProfile
import com.intellij.execution.executors.DefaultRunExecutor
import com.intellij.execution.runners.DefaultProgramRunner

/**
 * Run executor handler for [StrykeRunConfiguration].
 *
 * Extends [DefaultProgramRunner] (which provides the standard `doExecute`
 * implementation: save documents → `state.execute` → `RunContentBuilder`).
 * We only need to declare which executor / profile we handle.
 *
 * Do **not** extend `GenericProgramRunner` directly — its `doExecute` is
 * abstract in 2026.1+ and would surface as `AbstractMethodError` when the
 * user clicks Run.
 */
class StrykeProgramRunner : DefaultProgramRunner() {
    override fun getRunnerId(): String = "StrykeProgramRunner"

    override fun canRun(executorId: String, profile: RunProfile): Boolean {
        if (profile !is StrykeRunConfiguration) return false
        return executorId == DefaultRunExecutor.EXECUTOR_ID
    }
}
