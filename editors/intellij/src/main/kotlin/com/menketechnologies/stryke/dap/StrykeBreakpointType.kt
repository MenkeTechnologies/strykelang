package com.menketechnologies.stryke.dap

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.xdebugger.breakpoints.XLineBreakpointType
import com.intellij.xdebugger.breakpoints.XLineBreakpointTypeBase

/**
 * Line-breakpoint type for stryke `.stk` files.
 *
 * Registered as `stryke-line`. Available on every line of a `.stk` file —
 * the runtime decides at execution time whether the line is actually
 * reachable; we accept all lines to keep the gutter clean.
 */
class StrykeBreakpointType : XLineBreakpointTypeBase(
    "stryke-line",
    "Stryke Line Breakpoint",
    StrykeDebuggerEditorsProvider(),
) {
    override fun canPutAt(file: VirtualFile, line: Int, project: Project): Boolean =
        file.extension == "stk"

    override fun getPriority(): Int = 100
}
