package com.dipecs.collector.actions

import android.content.Context
import com.dipecs.collector.storage.EventRepository
import java.io.IOException
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

class AuthorizedActionSocketServer(
    private val context: Context,
    private val port: Int,
    private val authToken: String,
) {
    private val running = AtomicBoolean(false)
    private val failedAuthCount = AtomicInteger(0)
    private val rejectUntilMs = AtomicLong(0)
    private val acceptExecutor = Executors.newSingleThreadExecutor()
    private val clientExecutor = Executors.newCachedThreadPool()
    @Volatile
    private var serverSocket: ServerSocket? = null

    fun start() {
        if (!running.compareAndSet(false, true)) {
            return
        }

        acceptExecutor.execute {
            try {
                val socket = ServerSocket(port, 16, InetAddress.getByName(LOOPBACK_HOST))
                serverSocket = socket
                EventRepository.recordInternal(
                    context,
                    "authorized_action_socket_started",
                    "AuthorizedAction socket listening",
                    JSONObject()
                        .put("host", LOOPBACK_HOST)
                        .put("port", port)
                        .put("auth", "required"),
                )

                while (running.get()) {
                    val client = try {
                        socket.accept()
                    } catch (error: SocketException) {
                        if (!running.get()) {
                            break
                        }
                        throw error
                    }
                    clientExecutor.execute {
                        client.use { handleClient(it) }
                    }
                }
            } catch (error: Throwable) {
                if (running.get()) {
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_failed",
                        error.message ?: error.javaClass.simpleName,
                        JSONObject()
                            .put("host", LOOPBACK_HOST)
                            .put("port", port),
                    )
                }
            } finally {
                serverSocket?.close()
                serverSocket = null
                running.set(false)
            }
        }
    }

    fun stop() {
        if (!running.compareAndSet(true, false)) {
            return
        }
        runCatching { serverSocket?.close() }
        clientExecutor.shutdownNow()
        EventRepository.recordInternal(
            context,
            "authorized_action_socket_stopped",
            "AuthorizedAction socket stopped",
            JSONObject()
                .put("host", LOOPBACK_HOST)
                .put("port", port),
        )
    }

    private fun handleClient(client: Socket) {
        val payload = try {
            readPayload(client).trim()
        } catch (error: PayloadTooLargeException) {
            recordAuthFailure()
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_payload_too_large",
                "AuthorizedAction socket payload exceeded size limit",
                JSONObject()
                    .put("port", port)
                    .put("maxBytes", MAX_PAYLOAD_CHARS),
            )
            return
        } catch (error: SocketTimeoutException) {
            recordAuthFailure()
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_timeout",
                "AuthorizedAction socket read timed out",
                JSONObject().put("port", port),
            )
            return
        } catch (error: IOException) {
            recordAuthFailure()
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_read_failed",
                error.message ?: "AuthorizedAction socket read failed",
                JSONObject().put("port", port),
            )
            return
        }

        if (payload.isBlank()) {
            recordAuthFailure()
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_empty",
                "AuthorizedAction socket received empty payload",
                JSONObject().put("port", port),
            )
            return
        }

        runCatching { JSONObject(payload) }
            .onSuccess { json ->
                if (isRateLimited()) {
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_rate_limited",
                        "AuthorizedAction socket auth temporarily rate limited",
                        JSONObject().put("port", port),
                    )
                    return@onSuccess
                }
                if (!isAuthorized(json)) {
                    recordAuthFailure()
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_rejected",
                        "AuthorizedAction socket auth failed",
                        JSONObject().put("port", port),
                    )
                    return@onSuccess
                }
                failedAuthCount.set(0)
                rejectUntilMs.set(0)
                val dispatched = ActionExecutorBridge.dispatchAuthorizedActionJson(
                    context,
                    json,
                    reason = "socket_authorized_action",
                )
                if (!dispatched) {
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_dispatch_failed",
                        "AuthorizedAction socket payload was authorized but not dispatched",
                        JSONObject().put("port", port),
                    )
                }
            }
            .onFailure { error ->
                recordAuthFailure()
                EventRepository.recordInternal(
                    context,
                    "authorized_action_socket_invalid_json",
                    error.message ?: "Invalid AuthorizedAction JSON",
                    JSONObject()
                        .put("payload", payload.take(2048))
                        .put("port", port),
                )
            }
    }

    private fun readPayload(client: Socket): String {
        client.soTimeout = SOCKET_READ_TIMEOUT_MS.toInt()
        val reader = InputStreamReader(client.getInputStream(), Charsets.UTF_8)
        val buffer = CharArray(READ_BUFFER_CHARS)
        val payload = StringBuilder()
        while (true) {
            val read = reader.read(buffer)
            if (read < 0) {
                return payload.toString()
            }
            payload.append(buffer, 0, read)
            if (payload.length > MAX_PAYLOAD_CHARS) {
                throw PayloadTooLargeException()
            }
        }
    }

    private fun isAuthorized(payload: JSONObject): Boolean {
        val supplied = payload.optString(AUTH_TOKEN_FIELD).takeIf { it.isNotBlank() }
            ?: return false
        return constantTimeEquals(supplied, authToken)
    }

    private fun constantTimeEquals(left: String, right: String): Boolean {
        val leftBytes = left.toByteArray(Charsets.UTF_8)
        val rightBytes = right.toByteArray(Charsets.UTF_8)
        return MessageDigest.isEqual(leftBytes, rightBytes)
    }

    private fun recordAuthFailure() {
        val failures = failedAuthCount.incrementAndGet()
        if (failures >= MAX_AUTH_FAILURES_BEFORE_BACKOFF) {
            rejectUntilMs.set(System.currentTimeMillis() + AUTH_BACKOFF_MS)
        }
    }

    private fun isRateLimited(): Boolean =
        System.currentTimeMillis() < rejectUntilMs.get()

    private class PayloadTooLargeException : IOException()

    companion object {
        const val LOOPBACK_HOST = "127.0.0.1"
        const val AUTH_TOKEN_FIELD = "auth_token"
        private const val MAX_PAYLOAD_CHARS = 64 * 1024
        private const val READ_BUFFER_CHARS = 4096
        private const val MAX_AUTH_FAILURES_BEFORE_BACKOFF = 5
        private val SOCKET_READ_TIMEOUT_MS = TimeUnit.SECONDS.toMillis(5)
        private val AUTH_BACKOFF_MS = TimeUnit.SECONDS.toMillis(30)
    }
}
