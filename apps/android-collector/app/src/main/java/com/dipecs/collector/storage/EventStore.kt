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
        return file.readLines()
            .asSequence()
            .filter { it.isNotBlank() }
            .takeLastCompat(limit)
            .mapNotNull { line -> runCatching { sanitizeForTrace(JSONObject(line)) }.getOrNull() }
            .toList()
    }

    fun readRecentLines(limit: Int): List<String> {
        val file = traceFile
        if (!file.exists()) {
            return emptyList()
        }
        return file.readLines()
            .asSequence()
            .filter { it.isNotBlank() }
            .takeLastCompat(limit)
            .map { line ->
                runCatching { sanitizeForTrace(JSONObject(line)).toString() }
                    .getOrElse { line }
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
        if (source.exists()) {
            val sanitized = source.readLines()
                .asSequence()
                .filter { it.isNotBlank() }
                .map { line ->
                    runCatching { sanitizeForTrace(JSONObject(line)).toString() }
                        .getOrElse { "" }
                }
                .filter { it.isNotBlank() }
                .joinToString(separator = "\n", postfix = "\n")
            target.writeText(sanitized)
        } else {
            target.writeText("")
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
    }
}
