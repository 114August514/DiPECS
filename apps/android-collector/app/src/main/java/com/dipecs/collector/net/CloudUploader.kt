package com.dipecs.collector.net

import android.content.Context
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import com.dipecs.collector.storage.EventStore
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.InetAddress
import java.net.URL
import java.util.concurrent.Executors

object CloudUploader {
    private const val RECENT_EVENT_LIMIT = 100
    private const val MAX_RESPONSE_BODY_CHARS = 16 * 1024
    private val executor = Executors.newSingleThreadExecutor()

    fun uploadRecent(
        context: Context,
        reason: String = "manual",
        requireUploadEnabled: Boolean = false,
    ) {
        val appContext = context.applicationContext
        executor.execute {
            if (requireUploadEnabled && !CollectorPreferences.isUploadEnabled(appContext)) {
                EventRepository.recordInternal(
                    appContext,
                    "upload_skipped",
                    "Periodic upload disabled",
                    JSONObject().put("reason", reason),
                )
                return@execute
            }

            val endpoint = CollectorPreferences.endpoint(appContext)
            if (endpoint.isBlank()) {
                EventRepository.recordInternal(appContext, "upload_skipped", "No endpoint configured")
                return@execute
            }

            val mode = CollectorPreferences.uploadMode(appContext)
            val events = EventStore(appContext).readRecent(RECENT_EVENT_LIMIT)
            if (events.isEmpty()) {
                EventRepository.recordInternal(appContext, "upload_skipped", "No events to upload")
                return@execute
            }

            val payload = JSONObject()
                .put("schema", "dipecs.collector.v1")
                .put("mode", mode)
                .put("reason", reason)
                .put("generatedAtMs", System.currentTimeMillis())
                .put("events", JSONArray(events))

            runCatching {
                postJson(endpoint, payload, bearerToken = tokenForMode(appContext, mode))
            }.onSuccess { response ->
                EventRepository.recordInternal(
                    appContext,
                    "upload_success",
                    "Uploaded ${events.size} events",
                    JSONObject()
                        .put("mode", mode)
                        .put("httpCode", response.code)
                        .put("responseBytes", response.body.toByteArray(Charsets.UTF_8).size),
                )
            }.onFailure { error ->
                EventRepository.recordInternal(
                    appContext,
                    "upload_failed",
                    error.message ?: error.javaClass.simpleName,
                    JSONObject().put("mode", mode),
                )
            }
        }
    }

    private fun tokenForMode(context: Context, mode: String): String? {
        if (mode != CollectorPreferences.MODE_LLM) {
            return null
        }
        return CollectorPreferences.apiKey(context).ifBlank { null }
    }

    private fun postJson(endpoint: String, payload: JSONObject, bearerToken: String?): HttpResponse {
        val url = validateUploadEndpoint(endpoint)

        val connection = (url.openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            connectTimeout = 10_000
            readTimeout = 20_000
            doOutput = true
            instanceFollowRedirects = false
            setRequestProperty("Content-Type", "application/json; charset=utf-8")
            setRequestProperty("Accept", "application/json")
            if (!bearerToken.isNullOrBlank()) {
                setRequestProperty("Authorization", "Bearer $bearerToken")
            }
        }

        try {
            val bytes = payload.toString().toByteArray(Charsets.UTF_8)
            connection.outputStream.use { stream -> stream.write(bytes) }

            val code = connection.responseCode
            val body = runCatching {
                val input = if (code in 200..299) connection.inputStream else connection.errorStream
                input?.bufferedReader()?.use { reader ->
                    val buffer = CharArray(1024)
                    val body = StringBuilder()
                    while (body.length < MAX_RESPONSE_BODY_CHARS) {
                        val read = reader.read(buffer, 0, minOf(buffer.size, MAX_RESPONSE_BODY_CHARS - body.length))
                        if (read < 0) {
                            break
                        }
                        body.append(buffer, 0, read)
                    }
                    body.toString()
                } ?: ""
            }.getOrDefault("")

            if (code !in 200..299) {
                error("Upload failed with HTTP $code")
            }
            return HttpResponse(code, body)
        } finally {
            connection.disconnect()
        }
    }

    private fun validateUploadEndpoint(endpoint: String): URL {
        val url = URL(endpoint)
        require(url.protocol == "https") { "Upload endpoint must use HTTPS" }
        val host = url.host?.lowercase().orEmpty()
        require(host.isNotBlank()) { "Upload endpoint host is blank" }
        require(!isBlockedHostName(host)) { "Upload endpoint host is not allowed" }
        val addresses = InetAddress.getAllByName(host)
        require(addresses.isNotEmpty()) { "Upload endpoint host did not resolve" }
        require(addresses.none { it.isBlockedAddress() }) {
            "Upload endpoint resolved to a private or local address"
        }
        return url
    }

    private fun isBlockedHostName(host: String): Boolean =
        host == "localhost" || host.endsWith(".localhost")

    private fun InetAddress.isBlockedAddress(): Boolean {
        val bytes = address
        val uniqueLocalIpv6 = bytes.size == 16 && (bytes[0].toInt() and 0xfe) == 0xfc
        return isAnyLocalAddress ||
            isLoopbackAddress ||
            isLinkLocalAddress ||
            isSiteLocalAddress ||
            isMulticastAddress ||
            uniqueLocalIpv6
    }

    private data class HttpResponse(val code: Int, val body: String)
}
