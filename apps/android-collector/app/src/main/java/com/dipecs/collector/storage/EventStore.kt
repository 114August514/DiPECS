package com.dipecs.collector.storage

import android.content.Context
import com.dipecs.collector.model.CollectorEvent
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
            traceFile.appendText(event.toJson().toString() + "\n")
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
            .mapNotNull { line -> runCatching { JSONObject(line) }.getOrNull() }
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
            source.copyTo(target, overwrite = true)
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
    }
}
