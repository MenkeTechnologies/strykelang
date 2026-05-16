package com.menketechnologies.stryke.dap

import com.intellij.xdebugger.frame.XExecutionStack
import com.intellij.xdebugger.frame.XStackFrame
import com.intellij.xdebugger.frame.XSuspendContext

class StrykeSuspendContext(private val stack: StrykeExecutionStack) : XSuspendContext() {
    override fun getActiveExecutionStack(): XExecutionStack = stack
}

/**
 * Holds the frames captured at the most recent pause. No async fetch — all
 * frames + variables are pre-fetched at the `stopped` event in
 * [StrykeDebugProcess.onStopped] and stored on this stack. The IDE asks
 * `computeStackFrames`, we deliver everything immediately.
 */
class StrykeExecutionStack : XExecutionStack("Main") {

    @Volatile private var frames: List<StrykeStackFrame> = emptyList()

    fun setFrames(newFrames: List<StrykeStackFrame>) {
        frames = newFrames
    }

    override fun getTopFrame(): XStackFrame? = frames.firstOrNull()

    override fun computeStackFrames(firstFrameIndex: Int, container: XStackFrameContainer) {
        val slice = if (firstFrameIndex <= 0) frames else frames.drop(firstFrameIndex)
        container.addStackFrames(slice, true)
    }
}
