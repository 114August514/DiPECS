package com.dipecs.collector.actions

import android.content.Context
import android.net.Uri
import com.dipecs.collector.storage.EventRepository
import java.io.File
import java.io.FileOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.Executors
import org.json.JSONObject

object AccessibleContentPrefetcher {
    private const val CONNECT_TIMEOUT_MS = 10_000
    private const val READ_TIMEOUT_MS = 20_000
    private const val MAX_DOWNLOAD_BYTES = 2 * 1024 * 1024
    private val executor = Executors.newSingleThreadExecutor()

    fun enqueue(context: Context, rawTarget: String, reason: String = "manual") {
        val appContext = context.applicationContext
        executor.execute {
            val startedAtMs = System.currentTimeMillis()
            val parsedTarget = runCatching { PrefetchTarget.parse(rawTarget) }.getOrElse { error ->
                EventRepository.recordInternal(
                    appContext,
                    "prefetch_rejected",
                    error.message ?: "Invalid prefetch target",
                    JSONObject()
                        .put("target", rawTarget)
                        .put("reason", reason),
                )
                return@execute
            }

            EventRepository.recordInternal(
                appContext,
                "prefetch_started",
                "Prefetch started",
                JSONObject()
                    .put("target", parsedTarget.raw)
                    .put("reason", reason)
                    .put("kind", parsedTarget.kind),
            )

            runCatching {
                when (parsedTarget.kind) {
                    "url" -> prefetchUrl(appContext, parsedTarget)
                    "uri" -> prefetchUri(appContext, parsedTarget)
                    else -> error("Unsupported target kind: ${parsedTarget.kind}")
                }
            }.onSuccess { result ->
                EventRepository.recordInternal(
                    appContext,
                    "prefetch_succeeded",
                    "Prefetch completed",
                    JSONObject()
                        .put("target", parsedTarget.raw)
                        .put("reason", reason)
                        .put("kind", parsedTarget.kind)
                        .put("cachePath", result.cacheFile.absolutePath)
                        .put("bytes", result.bytes)
                        .put("contentType", result.contentType ?: JSONObject.NULL)
                        .put("durationMs", System.currentTimeMillis() - startedAtMs),
                )
            }.onFailure { error ->
                EventRepository.recordInternal(
                    appContext,
                    "prefetch_failed",
                    error.message ?: error.javaClass.simpleName,
                    JSONObject()
                        .put("target", parsedTarget.raw)
                        .put("reason", reason)
                        .put("kind", parsedTarget.kind)
                        .put("durationMs", System.currentTimeMillis() - startedAtMs),
                )
            }
        }
    }

    private fun prefetchUrl(context: Context, target: PrefetchTarget): PrefetchResult {
        val cacheDir = File(context.cacheDir, "prefetch")
        if (!cacheDir.exists()) {
            cacheDir.mkdirs()
        }
        val cacheFile = File(cacheDir, target.cacheFileName())

        val connection = (URL(target.value).openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            connectTimeout = CONNECT_TIMEOUT_MS
            readTimeout = READ_TIMEOUT_MS
            instanceFollowRedirects = true
            setRequestProperty("Accept", "*/*")
        }

        return try {
            val responseCode = connection.responseCode
            if (responseCode !in 200..299) {
                error("Prefetch failed with HTTP $responseCode")
            }

            val bytes = connection.inputStream.use { input ->
                FileOutputStream(cacheFile).use { output ->
                    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                    var total = 0L
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) {
                            break
                        }
                        total += read
                        if (total > MAX_DOWNLOAD_BYTES) {
                            error("Prefetch aborted: content exceeds ${MAX_DOWNLOAD_BYTES / 1024} KiB limit")
                        }
                        output.write(buffer, 0, read)
                    }
                    total
                }
            }

            PrefetchResult(
                cacheFile = cacheFile,
                bytes = bytes,
                contentType = connection.contentType,
            )
        } catch (error: Throwable) {
            cacheFile.delete()
            throw error
        } finally {
            connection.disconnect()
        }
    }

    private fun prefetchUri(context: Context, target: PrefetchTarget): PrefetchResult {
        val uri = Uri.parse(target.value)
        require(uri.scheme == "content") { "Only content:// URI prefetch targets are supported" }

        val cacheDir = File(context.cacheDir, "prefetch")
        if (!cacheDir.exists()) {
            cacheDir.mkdirs()
        }
        val cacheFile = File(cacheDir, target.cacheFileName())
        val contentType = context.contentResolver.getType(uri)

        return try {
            val bytes = context.contentResolver.openInputStream(uri)?.use { input ->
                FileOutputStream(cacheFile).use { output ->
                    val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                    var total = 0L
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) {
                            break
                        }
                        total += read
                        if (total > MAX_DOWNLOAD_BYTES) {
                            error("Prefetch aborted: content exceeds ${MAX_DOWNLOAD_BYTES / 1024} KiB limit")
                        }
                        output.write(buffer, 0, read)
                    }
                    total
                }
            } ?: error("Unable to open URI for reading")

            PrefetchResult(
                cacheFile = cacheFile,
                bytes = bytes,
                contentType = contentType,
            )
        } catch (error: Throwable) {
            cacheFile.delete()
            throw error
        }
    }

    internal data class PrefetchTarget(
        val raw: String,
        val kind: String,
        val value: String,
    ) {
        fun cacheFileName(): String {
            val digest = MessageDigest.getInstance("SHA-256")
                .digest(value.toByteArray(Charsets.UTF_8))
                .joinToString(separator = "") { byte -> "%02x".format(byte) }
            val extension = value.substringAfterLast('/', "")
                .substringAfterLast('.', "")
                .takeIf { it.length in 1..8 && it.all(Char::isLetterOrDigit) }
            return if (extension != null) {
                "$digest.$extension"
            } else {
                digest
            }
        }

        companion object {
            fun parse(rawTarget: String): PrefetchTarget {
                val trimmed = rawTarget.trim()
                require(trimmed.isNotBlank()) { "Prefetch target is blank" }

                val separatorIndex = trimmed.indexOf(':')
                require(separatorIndex > 0) {
                    "Prefetch target must use '<kind>:<value>' format"
                }

                val kind = trimmed.substring(0, separatorIndex).lowercase()
                val value = trimmed.substring(separatorIndex + 1).trim()
                require(value.isNotBlank()) { "Prefetch target value is blank" }

                return when (kind) {
                    "url" -> {
                        val normalizedValue = if (value.startsWith("http://") || value.startsWith("https://")) {
                            value
                        } else {
                            error("Only http:// or https:// URL prefetch targets are supported")
                        }
                        PrefetchTarget(trimmed, kind, normalizedValue)
                    }
                    "uri" -> {
                        require(value.startsWith("content://")) {
                            "Only content:// URI prefetch targets are supported"
                        }
                        PrefetchTarget(trimmed, kind, value)
                    }
                    else -> error("Unsupported prefetch target kind: $kind")
                }
            }
        }
    }

    private data class PrefetchResult(
        val cacheFile: File,
        val bytes: Long,
        val contentType: String?,
    )
}
