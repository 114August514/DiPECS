package com.dipecs.collector.actions

import com.dipecs.collector.actions.AuthorizedActionSocketServer.AuthVerdict
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class AuthorizedActionSocketServerTest {
    @Test
    fun validatePayloadRejectsMissingToken() {
        val payload = signedPayload(authToken = AUTH_TOKEN).apply {
            remove(AuthorizedActionSocketServer.AUTH_TOKEN_FIELD)
        }

        assertEquals(
            AuthVerdict.AUTH_TOKEN_MISSING_OR_INVALID,
            AuthorizedActionSocketServer.validatePayload(payload, AUTH_TOKEN),
        )
    }

    @Test
    fun validatePayloadRejectsWrongToken() {
        val payload = signedPayload(authToken = AUTH_TOKEN).apply {
            put(AuthorizedActionSocketServer.AUTH_TOKEN_FIELD, "wrong-token")
        }

        assertEquals(
            AuthVerdict.AUTH_TOKEN_MISSING_OR_INVALID,
            AuthorizedActionSocketServer.validatePayload(payload, AUTH_TOKEN),
        )
    }

    @Test
    fun validatePayloadRejectsExpiredSignatureWindow() {
        val issuedAtMs = System.currentTimeMillis() - 10 * 60 * 1000L
        val expiresAtMs = issuedAtMs + 60 * 1000L
        val payload = signedPayload(
            authToken = AUTH_TOKEN,
            issuedAtMs = issuedAtMs,
            expiresAtMs = expiresAtMs,
        )

        assertEquals(
            AuthVerdict.ACTION_WINDOW_MISSING_OR_EXPIRED,
            AuthorizedActionSocketServer.validatePayload(payload, AUTH_TOKEN),
        )
    }

    @Test
    fun validatePayloadAcceptsFreshCorrectSignature() {
        val payload = signedPayload(authToken = AUTH_TOKEN)

        assertEquals(
            AuthVerdict.ACTION_AUTHORIZED,
            AuthorizedActionSocketServer.validatePayload(payload, AUTH_TOKEN),
        )
    }

    private fun signedPayload(
        authToken: String,
        issuedAtMs: Long = System.currentTimeMillis(),
        expiresAtMs: Long = issuedAtMs + 60 * 1000L,
        actionType: String = ActionExecutorBridge.ACTION_TYPE_NO_OP,
        target: String = "",
        urgency: String = "Normal",
    ): JSONObject =
        JSONObject()
            .put(AuthorizedActionSocketServer.AUTH_TOKEN_FIELD, authToken)
            .put("issued_at_ms", issuedAtMs)
            .put("expires_at_ms", expiresAtMs)
            .put(
                "action",
                JSONObject()
                    .put("action_type", actionType)
                    .put("target", target)
                    .put("urgency", urgency),
            )
            .put(
                "action_signature",
                AuthorizedActionSocketServer.actionSignature(
                    authToken = authToken,
                    issuedAtMs = issuedAtMs,
                    expiresAtMs = expiresAtMs,
                    actionType = actionType,
                    target = target,
                    urgency = urgency,
                ),
            )

    private companion object {
        const val AUTH_TOKEN = "test-token-123"
    }
}
