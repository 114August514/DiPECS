package com.dipecs.collector.storage

import android.content.Context
import com.dipecs.collector.model.CollectorEvent
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

class EventStore(context: Context) {
    private val appContext = context.applicationContext

    val traceFile: File
        get() {
            val dir = File(appContext.filesDir, "traces")
            if (!dir.exists()) {
                dir.mkdirs()
            }
            return File(dir, "actions.jsonl")
        }

    fun append(event: CollectorEvent) {
        synchronized(LOCK) {
            traceFile.appendText(sanitizeForTrace(event.toJson()).toString() + "\n")
        }
    }

    fun readRecent(limit: Int): List<JSONObject> {
        val file = traceFile
        if (!file.exists()) {
            return emptyList()
        }

        return synchronized(LOCK) {
            // Keep memory bounded for long-running collectors: only the
            // ring-buffer helper holds the last `limit` nonblank rows.
            file.useLines { lines ->
                lines
                    .filter { it.isNotBlank() }
                    .takeLastCompat(limit)
                    .mapNotNull { line -> runCatching { sanitizeForTrace(JSONObject(line)) }.getOrNull() }
            }
        }
    }

    fun clear() {
        synchronized(LOCK) {
            val file = traceFile
            if (file.exists()) {
                file.writeText("")
            }
        }
    }

    fun exportToExternalFiles(): File {
        val source = traceFile
        val targetDir = File(appContext.getExternalFilesDir(null) ?: appContext.filesDir, "traces")
        if (!targetDir.exists()) {
            targetDir.mkdirs()
        }

        val target = File(targetDir, "actions.jsonl")

        synchronized(LOCK) {
            // Export is intentionally line-oriented. Large traces should not
            // need a full in-memory sanitized copy just to write the public file.
            target.bufferedWriter().use { writer ->
                if (source.exists()) {
                    source.useLines { lines ->
                        lines
                            .filter { it.isNotBlank() }
                            .forEach { line ->
                                val sanitized = runCatching {
                                    sanitizeForTrace(JSONObject(line)).toString()
                                }.getOrNull()
                                if (!sanitized.isNullOrBlank()) {
                                    writer.write(sanitized)
                                    writer.newLine()
                                }
                            }
                    }
                }
            }
        }
        return target
    }

    fun lineCount(): Int {
        val file = traceFile
        if (!file.exists()) {
            return 0
        }
        return file.useLines { lines -> lines.count() }
    }

    fun stats(): TraceStats {
        val file = traceFile
        if (!file.exists()) {
            return TraceStats(fileSizeBytes = 0L)
        }

        var total = 0
        var rawEventRows = 0
        var rawEventNullRows = 0
        var parseErrors = 0
        var latestParseError: String? = null
        var latestTimestampMs: Long? = null
        var latestRawEventKind: String? = null
        val sourceCounts = linkedMapOf<String, Int>()
        val eventTypeCounts = linkedMapOf<String, Int>()
        val rawEventKindCounts = linkedMapOf<String, Int>()

        file.useLines { lines ->
            for (line in lines) {
                if (line.isBlank()) {
                    continue
                }
                total += 1
                val parsed = runCatching { JSONObject(line) }
                val event = parsed.getOrNull()
                if (event == null) {
                    parseErrors += 1
                    latestParseError = parsed.exceptionOrNull()?.message
                        ?: "Invalid JSON row"
                    continue
                }

                latestTimestampMs = maxOf(
                    latestTimestampMs ?: Long.MIN_VALUE,
                    event.optLong("timestampMs", Long.MIN_VALUE),
                ).takeIf { it != Long.MIN_VALUE }
                increment(sourceCounts, event.optString("source", "unknown").ifBlank { "unknown" })
                increment(eventTypeCounts, event.optString("eventType", "unknown").ifBlank { "unknown" })

                val rawEvent = event.optJSONObject("rawEvent")
                if (rawEvent != null) {
                    val keys = rawEvent.keys()
                    if (keys.hasNext()) {
                        val kind = keys.next()
                        rawEventRows += 1
                        latestRawEventKind = kind
                        increment(rawEventKindCounts, kind)
                    }
                } else {
                    rawEventNullRows += 1
                }
            }
        }

        return TraceStats(
            totalRows = total,
            fileSizeBytes = file.length(),
            rawEventRows = rawEventRows,
            rawEventNullRows = rawEventNullRows,
            parseErrors = parseErrors,
            latestParseError = latestParseError,
            latestTimestampMs = latestTimestampMs,
            latestRawEventKind = latestRawEventKind,
            sourceCounts = sourceCounts,
            eventTypeCounts = eventTypeCounts,
            rawEventKindCounts = rawEventKindCounts,
        )
    }

    private fun <T> Sequence<T>.takeLastCompat(count: Int): List<T> {
        if (count <= 0) {
            return emptyList()
        }
        val buffer = ArrayDeque<T>(count)
        for (item in this) {
            if (buffer.size == count) {
                buffer.removeFirst()
            }
            buffer.addLast(item)
        }
        return buffer.toList()
    }

    companion object {
        private val LOCK = Any()

        private val SENSITIVE_NULL_KEYS = setOf(
            "group_key",
            "key",
            "tag",
            "payload",
            "responseBody",
            "sourceText",
            "sourceContentDescription",
            "textItems",
            "windowTitle",
            "text",
            "target",
            "cachePath",
        )

        private val SENSITIVE_STRING_KEYS = setOf(
            "raw_title",
            "raw_text",
            "notification_key",
        )

        fun sanitizeForTrace(value: JSONObject): JSONObject =
            sanitizeObject(value)

        private fun sanitizeObject(value: JSONObject): JSONObject {
            val sanitized = JSONObject()
            val keys = value.keys()
            while (keys.hasNext()) {
                val key = keys.next()
                val original = value.opt(key)
                when {
                    key in SENSITIVE_NULL_KEYS -> sanitized.put(key, JSONObject.NULL)
                    key in SENSITIVE_STRING_KEYS -> sanitized.put(key, "")
                    original is JSONObject -> sanitized.put(key, sanitizeObject(original))
                    original is JSONArray -> sanitized.put(key, sanitizeArray(original))
                    else -> sanitized.put(key, original ?: JSONObject.NULL)
                }
            }
            return sanitized
        }

        private fun sanitizeArray(value: JSONArray): JSONArray {
            val sanitized = JSONArray()
            for (index in 0 until value.length()) {
                when (val item = value.opt(index)) {
                    is JSONObject -> sanitized.put(sanitizeObject(item))
                    is JSONArray -> sanitized.put(sanitizeArray(item))
                    else -> sanitized.put(item ?: JSONObject.NULL)
                }
            }
            return sanitized
        }

        private fun increment(counts: MutableMap<String, Int>, key: String) {
            counts[key] = (counts[key] ?: 0) + 1
        }
    }
}

data class TraceStats(
    val totalRows: Int = 0,
    val fileSizeBytes: Long = 0L,
    val rawEventRows: Int = 0,
    val rawEventNullRows: Int = 0,
    val parseErrors: Int = 0,
    val latestParseError: String? = null,
    val latestTimestampMs: Long? = null,
    val latestRawEventKind: String? = null,
    val sourceCounts: Map<String, Int> = emptyMap(),
    val eventTypeCounts: Map<String, Int> = emptyMap(),
    val rawEventKindCounts: Map<String, Int> = emptyMap(),
)
