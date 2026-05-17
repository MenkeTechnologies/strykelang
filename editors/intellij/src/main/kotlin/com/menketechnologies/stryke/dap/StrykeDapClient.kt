package com.menketechnologies.stryke.dap

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.openapi.diagnostic.Logger
import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.nio.charset.StandardCharsets
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * DAP client speaking Content-Length-framed JSON-RPC.
 *
 * **Byte-based** — `Content-Length` in the DAP spec is bytes, not chars.
 * Reading via `InputStreamReader` / `BufferedReader.readLine` causes desync
 * the moment any response body contains a multi-byte UTF-8 sequence (stryke
 * variable reprs frequently do — `≈`, `→`, `…`, box-drawing). After a
 * single misread the framing slides forever and every subsequent request
 * times out. This implementation reads raw bytes for both headers and bodies,
 * then UTF-8-decodes the JSON body.
 */
class StrykeDapClient(
    output: OutputStream,
    input: InputStream,
    private val onEvent: (event: String, body: JsonObject) -> Unit,
    private val onLog: (line: String) -> Unit = {},
) {
    private val output = output
    private val input = BufferedInputStream(input)
    private val seq = AtomicInteger(1)
    private val pending = ConcurrentHashMap<Int, AtomicReference<JsonObject?>>()
    private val pendingLatch = ConcurrentHashMap<Int, CountDownLatch>()
    private val readerThread: Thread

    @Volatile private var alive = true

    init {
        readerThread = Thread({
            try { runReader() } catch (e: Exception) { LOG.warn("DAP reader died", e) }
            alive = false
            pendingLatch.values.forEach { it.countDown() }
        }, "Stryke-DAP-Reader").apply {
            isDaemon = true
            start()
        }
    }

    fun isAlive(): Boolean = alive

    /**
     * Send a DAP request and block until response arrives.
     * Returns the `body` JsonObject (may be empty) or null if disconnected /
     * timed out.
     */
    fun request(command: String, arguments: JsonObject = JsonObject(), timeoutMs: Long = 10_000): JsonObject? {
        val s = seq.getAndIncrement()
        val msg = JsonObject().apply {
            addProperty("seq", s)
            addProperty("type", "request")
            addProperty("command", command)
            add("arguments", arguments)
        }
        val latch = CountDownLatch(1)
        val slot = AtomicReference<JsonObject?>()
        pendingLatch[s] = latch
        pending[s] = slot
        send(msg)
        latch.await(timeoutMs, java.util.concurrent.TimeUnit.MILLISECONDS)
        pending.remove(s)
        pendingLatch.remove(s)
        return slot.get()
    }

    /** Fire-and-forget — used for `disconnect` / `terminate` during shutdown. */
    fun requestAsync(command: String, arguments: JsonObject = JsonObject()) {
        val s = seq.getAndIncrement()
        val msg = JsonObject().apply {
            addProperty("seq", s)
            addProperty("type", "request")
            addProperty("command", command)
            add("arguments", arguments)
        }
        send(msg)
    }

    @Synchronized
    private fun send(msg: JsonObject) {
        if (!alive) return
        val body = msg.toString().toByteArray(StandardCharsets.UTF_8)
        val header = "Content-Length: ${body.size}\r\n\r\n".toByteArray(StandardCharsets.US_ASCII)
        try {
            output.write(header)
            output.write(body)
            output.flush()
            val cmd = msg.get("command")?.asString
            val seqStr = msg.get("seq")?.asString ?: msg.get("seq")?.asInt?.toString()
            com.menketechnologies.stryke.StrykeDebugLog.log(
                "dap",
                "→ seq=$seqStr type=${msg.get("type")?.asString} command=$cmd bytes=${body.size}",
            )
        } catch (e: Exception) {
            LOG.warn("DAP send failed", e)
            com.menketechnologies.stryke.StrykeDebugLog.log("dap", "send failed: ${e.message}")
            alive = false
        }
    }

    private fun runReader() {
        while (alive) {
            // Headers — read byte-by-byte until "\r\n\r\n".
            var contentLength = -1
            val headerBytes = ByteArrayOutputStream()
            var sawCRLFCRLF = false
            while (!sawCRLFCRLF) {
                val b = input.read()
                if (b < 0) { alive = false; return }
                headerBytes.write(b)
                val arr = headerBytes.toByteArray()
                val sz = arr.size
                if (sz >= 4 && arr[sz - 4] == 0x0d.toByte() && arr[sz - 3] == 0x0a.toByte()
                    && arr[sz - 2] == 0x0d.toByte() && arr[sz - 1] == 0x0a.toByte()) {
                    sawCRLFCRLF = true
                }
            }
            val headerText = String(headerBytes.toByteArray(), StandardCharsets.US_ASCII)
            for (line in headerText.split("\r\n")) {
                val idx = line.indexOf(':')
                if (idx > 0) {
                    val k = line.substring(0, idx).trim()
                    val v = line.substring(idx + 1).trim()
                    if (k.equals("Content-Length", ignoreCase = true)) {
                        contentLength = v.toIntOrNull() ?: -1
                    }
                }
            }
            if (contentLength <= 0) continue

            // Body — read exactly contentLength BYTES.
            val bodyBytes = ByteArray(contentLength)
            var off = 0
            while (off < contentLength) {
                val n = input.read(bodyBytes, off, contentLength - off)
                if (n < 0) { alive = false; return }
                off += n
            }
            val body = String(bodyBytes, StandardCharsets.UTF_8)
            onLog("← $body")
            val obj = try { JsonParser.parseString(body).asJsonObject } catch (_: Exception) { continue }
            val msgType = obj.get("type")?.asString
            when (msgType) {
                "response" -> {
                    val reqSeq = obj.get("request_seq")?.asInt ?: continue
                    val cmd = obj.get("command")?.asString
                    val success = obj.get("success")?.asBoolean
                    com.menketechnologies.stryke.StrykeDebugLog.log(
                        "dap",
                        "← response req_seq=$reqSeq command=$cmd success=$success bytes=$contentLength",
                    )
                    pending[reqSeq]?.set(obj.getAsJsonObject("body") ?: JsonObject())
                    pendingLatch[reqSeq]?.countDown()
                }
                "event" -> {
                    val event = obj.get("event")?.asString ?: continue
                    val eventBody = obj.getAsJsonObject("body") ?: JsonObject()
                    com.menketechnologies.stryke.StrykeDebugLog.log(
                        "dap",
                        "← event=$event bytes=$contentLength",
                    )
                    try { onEvent(event, eventBody) } catch (e: Exception) {
                        LOG.warn("event handler", e)
                        com.menketechnologies.stryke.StrykeDebugLog.log(
                            "dap",
                            "event handler threw: ${e.message}",
                        )
                    }
                }
            }
        }
    }

    fun close() {
        alive = false
        try { output.close() } catch (_: Exception) {}
    }

    companion object {
        private val LOG = Logger.getInstance(StrykeDapClient::class.java)
    }
}
