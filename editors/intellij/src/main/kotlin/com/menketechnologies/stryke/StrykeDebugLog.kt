package com.menketechnologies.stryke

import java.io.File
import java.io.PrintWriter
import java.io.FileWriter
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter

/**
 * Append-only debug log written to `/tmp/stryke-plugin.log` so plugin
 * diagnostics are tailable with `tail -f /tmp/stryke-plugin.log`
 * without having to find the idea.log location for the running IDE.
 *
 * Used by the LSP descriptor, refactoring handler, and rename handler
 * to surface "did this code path actually fire?" without forcing the
 * user to dig through 60 MB of idea.log.
 */
object StrykeDebugLog {
    private val LOG_FILE = File("/tmp/stryke-plugin.log")
    private val FMT = DateTimeFormatter.ofPattern("HH:mm:ss.SSS")
    private val LOCK = Any()

    fun log(tag: String, msg: String) {
        synchronized(LOCK) {
            try {
                PrintWriter(FileWriter(LOG_FILE, true)).use { w ->
                    w.println("[${LocalDateTime.now().format(FMT)}] [$tag] $msg")
                }
            } catch (_: Exception) {
                // Silent — debug log failures shouldn't propagate.
            }
        }
    }
}
