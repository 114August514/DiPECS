package com.dipecs.collector.actions

import android.content.Context
import com.dipecs.collector.storage.EventRepository
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.InetAddress
import java.net.ServerSocket
import java.net.SocketException
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import org.json.JSONObject

class AuthorizedActionSocketServer(
    private val context: Context,
    private val port: Int,
) {
    private val running = AtomicBoolean(false)
    private val acceptExecutor = Executors.newSingleThreadExecutor()
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
                        .put("port", port),
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
                    client.use { handleClient(it) }
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
        EventRepository.recordInternal(
            context,
            "authorized_action_socket_stopped",
            "AuthorizedAction socket stopped",
            JSONObject()
                .put("host", LOOPBACK_HOST)
                .put("port", port),
        )
    }

    private fun handleClient(client: java.net.Socket) {
        val payload = BufferedReader(InputStreamReader(client.getInputStream(), Charsets.UTF_8))
            .use { reader -> reader.readText().trim() }
        if (payload.isBlank()) {
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
                ActionExecutorBridge.dispatchAuthorizedActionJson(
                    context,
                    json,
                    reason = "socket_authorized_action",
                )
            }
            .onFailure { error ->
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

    companion object {
        const val LOOPBACK_HOST = "127.0.0.1"
    }
}
