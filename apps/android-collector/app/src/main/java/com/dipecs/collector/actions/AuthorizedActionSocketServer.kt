package com.dipecs.collector.actions

import android.content.Context
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import java.io.IOException
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.security.MessageDigest
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec
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
    private val clientExecutor = ThreadPoolExecutor(
        MAX_CLIENT_THREADS,
        MAX_CLIENT_THREADS,
        30L,
        TimeUnit.SECONDS,
        ArrayBlockingQueue(MAX_PENDING_CLIENTS),
    )
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
                CollectorPreferences.setActionSocketStatus(
                    context,
                    listening = true,
                    status = "listening on $LOOPBACK_HOST:$port",
                )
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
                    try {
                        clientExecutor.execute {
                            client.use { handleClient(it) }
                        }
                    } catch (_: RejectedExecutionException) {
                        client.close()
                        EventRepository.recordInternal(
                            context,
                            "authorized_action_socket_busy",
                            "AuthorizedAction socket client queue is full",
                            JSONObject().put("port", port),
                        )
                    }
                }
            } catch (error: Throwable) {
                if (running.get()) {
                    CollectorPreferences.setActionSocketStatus(
                        context,
                        listening = false,
                        status = error.message ?: error.javaClass.simpleName,
                    )
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
                CollectorPreferences.setActionSocketStatus(
                    context,
                    listening = false,
                    status = "stopped",
                )
            }
        }
    }

    fun stop() {
        if (!running.compareAndSet(true, false)) {
            return
        }
        runCatching { serverSocket?.close() }
        clientExecutor.shutdownNow()
        CollectorPreferences.setActionSocketStatus(
            context,
            listening = false,
            status = "stopped",
        )
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
        val startedAtNs = System.nanoTime()
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
            sendBridgeResponse(
                client,
                BridgeExecuteProtocol.STATUS_REJECTED,
                error = "empty payload",
                startedAtNs = startedAtNs,
            )
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
                if (BridgeExecuteProtocol.isExecuteEnvelope(json)) {
                    handleExecuteEnvelope(client, json, startedAtNs)
                    return@onSuccess
                }
                if (isRateLimited()) {
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_rate_limited",
                        "AuthorizedAction socket auth temporarily rate limited",
                        JSONObject().put("port", port),
                    )
                    return@onSuccess
                }
                when (validatePayload(json, authToken)) {
                    AuthVerdict.AUTH_TOKEN_MISSING_OR_INVALID -> {
                        recordAuthFailure()
                        EventRepository.recordInternal(
                            context,
                            "authorized_action_socket_rejected",
                            "AuthorizedAction socket auth failed",
                            JSONObject().put("port", port),
                        )
                        return@onSuccess
                    }
                    AuthVerdict.PING -> {
                        failedAuthCount.set(0)
                        rejectUntilMs.set(0)
                        sendPong(client)
                        EventRepository.recordInternal(
                            context,
                            "authorized_action_socket_ping",
                            "Ping received, pong sent",
                            JSONObject().put("port", port),
                        )
                        return@onSuccess
                    }
                    AuthVerdict.ACTION_WINDOW_MISSING_OR_EXPIRED -> {
                        recordAuthFailure()
                        EventRepository.recordInternal(
                            context,
                            "authorized_action_socket_stale",
                            "AuthorizedAction socket payload missing or expired freshness window",
                            JSONObject().put("port", port),
                        )
                        return@onSuccess
                    }
                    AuthVerdict.ACTION_SIGNATURE_INVALID -> {
                        recordAuthFailure()
                        EventRepository.recordInternal(
                            context,
                            "authorized_action_socket_bad_signature",
                            "AuthorizedAction socket payload signature failed",
                            JSONObject().put("port", port),
                        )
                        return@onSuccess
                    }
                    AuthVerdict.ACTION_AUTHORIZED -> Unit
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
                sendBridgeResponse(
                    client,
                    BridgeExecuteProtocol.STATUS_REJECTED,
                    error = "invalid json",
                    startedAtNs = startedAtNs,
                )
                EventRepository.recordInternal(
                    context,
                    "authorized_action_socket_invalid_json",
                    error.message ?: "Invalid AuthorizedAction JSON",
                    JSONObject()
                        .put("payloadBytes", payload.toByteArray(Charsets.UTF_8).size)
                        .put("port", port),
                )
            }
    }

    private fun handleExecuteEnvelope(
        client: Socket,
        json: JSONObject,
        startedAtNs: Long,
    ) {
        if (isRateLimited()) {
            sendBridgeResponse(
                client,
                BridgeExecuteProtocol.STATUS_REJECTED,
                error = "auth temporarily rate limited",
                startedAtNs = startedAtNs,
            )
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_rate_limited",
                "AuthorizedAction socket auth temporarily rate limited",
                JSONObject().put("port", port).put("protocol", "bridge_execute"),
            )
            return
        }

        when (val verified = BridgeExecuteProtocol.verifyExecuteEnvelope(json, authToken)) {
            is BridgeExecuteProtocol.Verification.Accepted -> {
                failedAuthCount.set(0)
                rejectUntilMs.set(0)
                val dispatchResult = runCatching {
                    ActionExecutorBridge.dispatchAuthorizedActionJson(
                        context,
                        verified.authorizedAction,
                        reason = "bridge_execute",
                    )
                }
                val dispatched = dispatchResult.getOrDefault(false)
                if (dispatched) {
                    val actionType = verified.authorizedAction
                        .optJSONObject("action")
                        ?.optString("action_type")
                        ?.takeIf { it.isNotBlank() }
                        ?: "unknown"
                    sendBridgeResponse(
                        client,
                        BridgeExecuteProtocol.STATUS_OK,
                        summary = "android_dispatched:$actionType",
                        startedAtNs = startedAtNs,
                    )
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_execute_ok",
                        "Bridge execute request dispatched",
                        JSONObject()
                            .put("port", port)
                            .put("actionType", actionType),
                    )
                } else {
                    sendBridgeResponse(
                        client,
                        BridgeExecuteProtocol.STATUS_ERROR,
                        error = dispatchResult.exceptionOrNull()?.message
                            ?: "authorized action was not dispatched",
                        startedAtNs = startedAtNs,
                    )
                    EventRepository.recordInternal(
                        context,
                        "authorized_action_socket_dispatch_failed",
                        dispatchResult.exceptionOrNull()?.message
                            ?: "Bridge execute request was authorized but not dispatched",
                        JSONObject().put("port", port),
                    )
                }
            }
            is BridgeExecuteProtocol.Verification.Rejected -> {
                recordAuthFailure()
                sendBridgeResponse(
                    client,
                    BridgeExecuteProtocol.STATUS_REJECTED,
                    error = verified.reason,
                    startedAtNs = startedAtNs,
                )
                EventRepository.recordInternal(
                    context,
                    "authorized_action_socket_execute_rejected",
                    "Bridge execute request rejected",
                    JSONObject()
                        .put("port", port)
                        .put("reason", verified.reason),
                )
            }
        }
    }

    private fun sendPong(client: Socket) {
        runCatching {
            val payload = """{"status":"ok","message":"pong"}"""
                .toByteArray(Charsets.UTF_8)
            val output = client.getOutputStream()
            output.write(payload)
            output.flush()
            client.shutdownOutput()
        }.onFailure { error ->
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_pong_failed",
                error.message ?: "Failed to send pong",
                JSONObject().put("port", port),
            )
        }
    }

    private fun sendBridgeResponse(
        client: Socket,
        status: String,
        summary: String? = null,
        error: String? = null,
        startedAtNs: Long,
    ) {
        runCatching {
            val response = BridgeExecuteProtocol.responseJson(
                status = status,
                summary = summary,
                error = error,
                latencyUs = TimeUnit.NANOSECONDS.toMicros(System.nanoTime() - startedAtNs),
            )
            val writer = OutputStreamWriter(client.getOutputStream(), Charsets.UTF_8)
            writer.write(response.toString())
            writer.flush()
        }.onFailure { failure ->
            EventRepository.recordInternal(
                context,
                "authorized_action_socket_response_failed",
                failure.message ?: "Failed to send bridge response",
                JSONObject()
                    .put("port", port)
                    .put("status", status),
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
            val text = payload.toString().trim()
            if (text.isNotEmpty() && runCatching { JSONObject(text) }.isSuccess) {
                return text
            }
        }
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

    internal enum class AuthVerdict {
        AUTH_TOKEN_MISSING_OR_INVALID,
        PING,
        ACTION_WINDOW_MISSING_OR_EXPIRED,
        ACTION_SIGNATURE_INVALID,
        ACTION_AUTHORIZED,
    }

    companion object {
        const val LOOPBACK_HOST = "127.0.0.1"
        const val AUTH_TOKEN_FIELD = "auth_token"
        private const val ISSUED_AT_FIELD = "issued_at_ms"
        private const val EXPIRES_AT_FIELD = "expires_at_ms"
        private const val ACTION_SIGNATURE_FIELD = "action_signature"
        private const val MAX_PAYLOAD_CHARS = 64 * 1024
        private const val READ_BUFFER_CHARS = 4096
        private const val MAX_CLIENT_THREADS = 4
        private const val MAX_PENDING_CLIENTS = 16
        private const val MAX_AUTH_FAILURES_BEFORE_BACKOFF = 5
        private val MAX_ACTION_TTL_MS = TimeUnit.MINUTES.toMillis(5)
        private val CLOCK_SKEW_MS = TimeUnit.SECONDS.toMillis(30)
        private val SOCKET_READ_TIMEOUT_MS = TimeUnit.SECONDS.toMillis(5)
        private val AUTH_BACKOFF_MS = TimeUnit.SECONDS.toMillis(30)

        internal fun validatePayload(payload: JSONObject, authToken: String): AuthVerdict {
            if (!isAuthorized(payload, authToken)) {
                return AuthVerdict.AUTH_TOKEN_MISSING_OR_INVALID
            }
            if (payload.optString("message_type") == "ping") {
                return AuthVerdict.PING
            }
            if (!hasFreshActionWindow(payload)) {
                return AuthVerdict.ACTION_WINDOW_MISSING_OR_EXPIRED
            }
            if (!hasValidActionSignature(payload, authToken)) {
                return AuthVerdict.ACTION_SIGNATURE_INVALID
            }
            return AuthVerdict.ACTION_AUTHORIZED
        }

        private fun isAuthorized(payload: JSONObject, authToken: String): Boolean {
            val supplied = payload.optString(AUTH_TOKEN_FIELD).takeIf { it.isNotBlank() }
                ?: return false
            return constantTimeEquals(supplied, authToken)
        }

        private fun hasFreshActionWindow(payload: JSONObject): Boolean {
            if (!payload.has("action") || payload.isNull("action")) {
                return false
            }
            val issuedAtMs = payload.optLong(ISSUED_AT_FIELD, 0L)
            val expiresAtMs = payload.optLong(EXPIRES_AT_FIELD, 0L)
            if (issuedAtMs <= 0L || expiresAtMs <= 0L || expiresAtMs <= issuedAtMs) {
                return false
            }
            if (expiresAtMs - issuedAtMs > MAX_ACTION_TTL_MS) {
                return false
            }
            val now = System.currentTimeMillis()
            return now + CLOCK_SKEW_MS >= issuedAtMs && now - CLOCK_SKEW_MS <= expiresAtMs
        }

        private fun hasValidActionSignature(payload: JSONObject, authToken: String): Boolean {
            val supplied = payload.optString(ACTION_SIGNATURE_FIELD).takeIf { it.isNotBlank() }
                ?: return false
            val issuedAtMs = payload.optLong(ISSUED_AT_FIELD, 0L)
            val expiresAtMs = payload.optLong(EXPIRES_AT_FIELD, 0L)
            val action = payload.optJSONObject("action") ?: return false
            val actionType = action.optString("action_type").takeIf { it.isNotBlank() }
                ?: return false
            val target = if (action.has("target") && !action.isNull("target")) {
                action.optString("target")
            } else {
                ""
            }
            val urgency = action.optString("urgency").takeIf { it.isNotBlank() }
                ?: return false
            val canonical = canonicalActionSignatureInput(
                issuedAtMs = issuedAtMs,
                expiresAtMs = expiresAtMs,
                actionType = actionType,
                target = target,
                urgency = urgency,
            )
            val expected = hmacSha256Hex(authToken, canonical)
            return constantTimeEquals(supplied.lowercase(), expected)
        }

        internal fun actionSignature(
            authToken: String,
            issuedAtMs: Long,
            expiresAtMs: Long,
            actionType: String,
            target: String,
            urgency: String,
        ): String =
            hmacSha256Hex(
                authToken,
                canonicalActionSignatureInput(
                    issuedAtMs = issuedAtMs,
                    expiresAtMs = expiresAtMs,
                    actionType = actionType,
                    target = target,
                    urgency = urgency,
                ),
            )

        private fun canonicalActionSignatureInput(
            issuedAtMs: Long,
            expiresAtMs: Long,
            actionType: String,
            target: String,
            urgency: String,
        ): String =
            "dipecs.android.action.v1\n" +
                "issued_at_ms:$issuedAtMs\n" +
                "expires_at_ms:$expiresAtMs\n" +
                "action_type:${actionType.length}:$actionType\n" +
                "target:${target.length}:$target\n" +
                "urgency:${urgency.length}:$urgency"

        private fun hmacSha256Hex(key: String, message: String): String {
            val mac = Mac.getInstance("HmacSHA256")
            mac.init(SecretKeySpec(key.toByteArray(Charsets.UTF_8), "HmacSHA256"))
            return mac.doFinal(message.toByteArray(Charsets.UTF_8))
                .joinToString(separator = "") { byte -> "%02x".format(byte) }
        }

        private fun constantTimeEquals(left: String, right: String): Boolean {
            val leftBytes = left.toByteArray(Charsets.UTF_8)
            val rightBytes = right.toByteArray(Charsets.UTF_8)
            return MessageDigest.isEqual(leftBytes, rightBytes)
        }
    }
}

internal object BridgeExecuteProtocol {
    const val MESSAGE_TYPE_EXECUTE = "execute"
    const val STATUS_OK = "ok"
    const val STATUS_REJECTED = "rejected"
    const val STATUS_ERROR = "error"

    private const val MESSAGE_TYPE_FIELD = "message_type"
    private const val AUTH_FIELD = "auth"
    private const val HMAC_FIELD = "hmac_sha256"
    private const val ACTION_FIELD = "action"
    private const val ISSUED_AT_FIELD = "issued_at_ms"
    private const val EXPIRES_AT_FIELD = "expires_at_ms"
    private val MAX_ENVELOPE_TTL_MS = TimeUnit.MINUTES.toMillis(5)
    private val ENVELOPE_CLOCK_SKEW_MS = TimeUnit.SECONDS.toMillis(30)

    sealed class Verification {
        data class Accepted(val authorizedAction: JSONObject) : Verification()
        data class Rejected(val reason: String) : Verification()
    }

    fun isExecuteEnvelope(payload: JSONObject): Boolean =
        payload.optString(MESSAGE_TYPE_FIELD) == MESSAGE_TYPE_EXECUTE

    fun verifyExecuteEnvelope(payload: JSONObject, authToken: String): Verification {
        val actionJson = (payload.opt(ACTION_FIELD) as? String)?.takeIf { it.isNotBlank() }
            ?: return Verification.Rejected("missing action")
        val issuedAtMs = payload.optLong(ISSUED_AT_FIELD, 0L)
        val expiresAtMs = payload.optLong(EXPIRES_AT_FIELD, 0L)
        validateFreshnessWindow(issuedAtMs, expiresAtMs)?.let { reason ->
            return Verification.Rejected(reason)
        }
        val suppliedHmac = payload.optJSONObject(AUTH_FIELD)
            ?.optString(HMAC_FIELD)
            ?.takeIf { it.isNotBlank() }
            ?: return Verification.Rejected("missing hmac")
        val expectedHmac = hmacSha256Hex(
            authToken,
            canonicalExecuteEnvelopeInput(issuedAtMs, expiresAtMs, actionJson),
        )
        if (!constantTimeEquals(suppliedHmac.lowercase(), expectedHmac)) {
            return Verification.Rejected("bad hmac")
        }
        val authorizedAction = runCatching { JSONObject(actionJson) }.getOrElse {
            return Verification.Rejected("invalid action json")
        }
        if (authorizedAction.optJSONObject(ACTION_FIELD) == null) {
            return Verification.Rejected("authorized action missing action object")
        }
        return Verification.Accepted(authorizedAction)
    }

    fun canonicalExecuteEnvelopeInput(
        issuedAtMs: Long,
        expiresAtMs: Long,
        actionJson: String,
    ): String =
        "dipecs.android.bridge.execute.v1\n" +
            "issued_at_ms:$issuedAtMs\n" +
            "expires_at_ms:$expiresAtMs\n" +
            "action:${actionJson.toByteArray(Charsets.UTF_8).size}:$actionJson"

    private fun validateFreshnessWindow(issuedAtMs: Long, expiresAtMs: Long): String? {
        if (issuedAtMs <= 0L || expiresAtMs <= 0L || expiresAtMs <= issuedAtMs) {
            return "missing or invalid freshness window"
        }
        if (expiresAtMs - issuedAtMs > MAX_ENVELOPE_TTL_MS) {
            return "freshness window too long"
        }
        val now = System.currentTimeMillis()
        if (now + ENVELOPE_CLOCK_SKEW_MS < issuedAtMs || now - ENVELOPE_CLOCK_SKEW_MS > expiresAtMs) {
            return "expired freshness window"
        }
        return null
    }

    fun responseJson(
        status: String,
        summary: String? = null,
        error: String? = null,
        latencyUs: Long? = null,
    ): JSONObject {
        val response = JSONObject().put("status", status)
        if (summary != null) {
            response.put("summary", summary)
        }
        if (latencyUs != null) {
            response.put("latency_us", latencyUs)
        }
        if (error != null) {
            response.put("error", error)
        }
        return response
    }

    fun hmacSha256Hex(key: String, message: String): String {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(key.toByteArray(Charsets.UTF_8), "HmacSHA256"))
        return mac.doFinal(message.toByteArray(Charsets.UTF_8))
            .joinToString(separator = "") { byte -> "%02x".format(byte) }
    }

    private fun constantTimeEquals(left: String, right: String): Boolean {
        val leftBytes = left.toByteArray(Charsets.UTF_8)
        val rightBytes = right.toByteArray(Charsets.UTF_8)
        return MessageDigest.isEqual(leftBytes, rightBytes)
    }
}
