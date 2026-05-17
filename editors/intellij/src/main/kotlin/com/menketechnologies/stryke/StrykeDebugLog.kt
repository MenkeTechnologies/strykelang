package com.menketechnologies.stryke

import java.io.File
import java.io.PrintWriter
import java.io.FileWriter
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter

/**
 * Append-only debug log written under the standard stryke state dir
 * (`~/.stryke/stryke-plugin.log`, or `$STRYKE_HOME/stryke-plugin.log` when
 * that env var is set). Tail with `tail -f ~/.stryke/stryke-plugin.log`.
 *
 * Hardcoding `/tmp/stryke-plugin.log` was an antipattern — it wasn't
 * persistent across reboots on some systems, didn't follow the
 * `~/.stryke/` convention every other stryke surface uses (`audit.log`,
 * `perf.sqlite`, `history`, `config.toml`, `scripts.rkyv`, `store/`), and
 * was awkward in multi-user setups.
 *
 * Used by the LSP descriptor, refactoring handler, rename handler, and
 * DAP client to surface "did this code path actually fire?" without
 * forcing the user to dig through 60 MB of idea.log.
 */
object StrykeDebugLog {
    private val LOG_FILE: File by lazy { resolveLogFile() }
    private val FMT = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss.SSS")
    private val LOCK = Any()

    /**
     * Resolve the log destination. Honors `$STRYKE_HOME` so users who
     * relocate the stryke state dir via env get a single coherent set
     * of paths. Falls back to `~/.stryke/stryke-plugin.log`. Creates the
     * parent directory if it doesn't exist; failures fall back to
     * `/tmp/stryke-plugin.log` for diagnostic continuity.
     */
    private fun resolveLogFile(): File {
        val envHome = System.getenv("STRYKE_HOME")
        val base = if (!envHome.isNullOrBlank()) {
            File(envHome)
        } else {
            File(System.getProperty("user.home"), ".stryke")
        }
        return try {
            if (!base.exists()) base.mkdirs()
            File(base, "stryke-plugin.log")
        } catch (_: Exception) {
            File("/tmp/stryke-plugin.log")
        }
    }

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

    /** Path the next [log] call will append to. Useful for status / about UIs. */
    fun path(): String = LOG_FILE.absolutePath
}
