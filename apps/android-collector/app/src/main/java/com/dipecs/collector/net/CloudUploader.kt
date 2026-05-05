package com.dipecs.collector.net

import android.content.Context
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import com.dipecs.collector.storage.EventStore
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

object CloudUploader {
    private const val RECENT_EVENT_LIMIT = 100
    private val executor = Executors.newSingleThreadExecutor()

    fun uploadRecent(context: Context, reason: String = "manual") {
        val appContext = context.applicationContext
        executor.execute {
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
                        .put("responseBody", response.body.take(4096)),
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
        val connection = (URL(endpoint).openConnection() as HttpURLConnection).apply {
            requestMethod = "POST"
            connectTimeout = 10_000
            readTimeout = 20_000
            doOutput = true
            setRequestProperty("Content-Type", "application/json; charset=utf-8")
            setRequestProperty("Accept", "application/json")
            if (!bearerToken.isNullOrBlank()) {
                setRequestProperty("Authorization", "Bearer $bearerToken")
            }
        }

        val bytes = payload.toString().toByteArray(Charsets.UTF_8)
        connection.outputStream.use { stream -> stream.write(bytes) }

        val code = connection.responseCode
        val body = runCatching {
            val input = if (code in 200..299) connection.inputStream else connection.errorStream
            input?.bufferedReader()?.use { it.readText() } ?: ""
        }.getOrDefault("")
        connection.disconnect()

        if (code !in 200..299) {
            error("Upload failed with HTTP $code: ${body.take(512)}")
        }
        return HttpResponse(code, body)
    }

    private data class HttpResponse(val code: Int, val body: String)
}
