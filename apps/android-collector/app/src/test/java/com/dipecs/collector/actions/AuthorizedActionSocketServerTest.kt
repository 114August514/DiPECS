package com.dipecs.collector.actions

import com.dipecs.collector.actions.AuthorizedActionSocketServer.AuthVerdict
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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

    @Test
    fun executeEnvelopeAcceptsValidHmacAndParsesAuthorizedAction() {
        val actionJson = authorizedActionJson()
        val envelope = executeEnvelope(actionJson, AUTH_TOKEN)

        val verified = BridgeExecuteProtocol.verifyExecuteEnvelope(envelope, AUTH_TOKEN)

        assertTrue(verified is BridgeExecuteProtocol.Verification.Accepted)
        val accepted = verified as BridgeExecuteProtocol.Verification.Accepted
        assertEquals(
            "PrefetchFile",
            accepted.authorizedAction.getJSONObject("action").getString("action_type"),
        )
        assertEquals(
            "url:https://example.test/feed.json",
            accepted.authorizedAction.getJSONObject("action").getString("target"),
        )
    }

    @Test
    fun executeEnvelopeRejectsBadHmac() {
        val now = System.currentTimeMillis()
        val envelope = JSONObject()
            .put("message_type", BridgeExecuteProtocol.MESSAGE_TYPE_EXECUTE)
            .put("issued_at_ms", now)
            .put("expires_at_ms", now + 60_000L)
            .put("auth", JSONObject().put("hmac_sha256", "00"))
            .put("action", authorizedActionJson())

        val verified = BridgeExecuteProtocol.verifyExecuteEnvelope(envelope, AUTH_TOKEN)

        assertTrue(verified is BridgeExecuteProtocol.Verification.Rejected)
        assertEquals(
            "bad hmac",
            (verified as BridgeExecuteProtocol.Verification.Rejected).reason,
        )
    }

    @Test
    fun executeEnvelopeRejectsNonObjectActionPayload() {
        val actionJson = "\"not-an-object\""
        val envelope = executeEnvelope(actionJson, AUTH_TOKEN)

        val verified = BridgeExecuteProtocol.verifyExecuteEnvelope(envelope, AUTH_TOKEN)

        assertTrue(verified is BridgeExecuteProtocol.Verification.Rejected)
        assertEquals(
            "invalid action json",
            (verified as BridgeExecuteProtocol.Verification.Rejected).reason,
        )
    }

    @Test
    fun executeEnvelopeRejectsActionThatIsNotAString() {
        val envelope = JSONObject()
            .put("message_type", BridgeExecuteProtocol.MESSAGE_TYPE_EXECUTE)
            .put("auth", JSONObject().put("hmac_sha256", "00"))
            .put("action", JSONObject().put("action_type", "PrefetchFile"))

        val verified = BridgeExecuteProtocol.verifyExecuteEnvelope(envelope, AUTH_TOKEN)

        assertTrue(verified is BridgeExecuteProtocol.Verification.Rejected)
        assertEquals(
            "missing action",
            (verified as BridgeExecuteProtocol.Verification.Rejected).reason,
        )
    }

    @Test
    fun executeEnvelopeRejectsExpiredFreshnessWindow() {
        val actionJson = authorizedActionJson()
        val issuedAtMs = System.currentTimeMillis() - 120_000L
        val expiresAtMs = issuedAtMs + 60_000L
        val canonical = BridgeExecuteProtocol.canonicalExecuteEnvelopeInput(
            issuedAtMs,
            expiresAtMs,
            actionJson,
        )
        val envelope = JSONObject()
            .put("message_type", BridgeExecuteProtocol.MESSAGE_TYPE_EXECUTE)
            .put("issued_at_ms", issuedAtMs)
            .put("expires_at_ms", expiresAtMs)
            .put(
                "auth",
                JSONObject().put(
                    "hmac_sha256",
                    BridgeExecuteProtocol.hmacSha256Hex(AUTH_TOKEN, canonical),
                ),
            )
            .put("action", actionJson)

        val verified = BridgeExecuteProtocol.verifyExecuteEnvelope(envelope, AUTH_TOKEN)

        assertTrue(verified is BridgeExecuteProtocol.Verification.Rejected)
        assertEquals(
            "expired freshness window",
            (verified as BridgeExecuteProtocol.Verification.Rejected).reason,
        )
    }

    @Test
    fun bridgeExecuteResponseUsesExpectedFields() {
        val response = BridgeExecuteProtocol.responseJson(
            status = BridgeExecuteProtocol.STATUS_OK,
            summary = "android_dispatched:PrefetchFile",
            latencyUs = 42,
        )

        assertEquals("ok", response.getString("status"))
        assertEquals("android_dispatched:PrefetchFile", response.getString("summary"))
        assertEquals(42, response.getLong("latency_us"))
        assertTrue(!response.has("error"))
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

    private fun authorizedActionJson(): String =
        JSONObject()
            .put("intent_id", "intent-1")
            .put(
                "coord",
                JSONObject()
                    .put("window_ordinal", 0)
                    .put("intent_ordinal", 0)
                    .put("action_ordinal", 0),
            )
            .put(
                "action",
                JSONObject()
                    .put("action_type", "PrefetchFile")
                    .put("target", "url:https://example.test/feed.json")
                    .put("urgency", "Immediate"),
            )
            .put("effect", "LocalCacheWrite")
            .put("authorized_at_ms", 1000)
            .toString()

    private fun executeEnvelope(actionJson: String, authToken: String): JSONObject {
        val issuedAtMs = System.currentTimeMillis()
        val expiresAtMs = issuedAtMs + 60_000L
        val canonical = BridgeExecuteProtocol.canonicalExecuteEnvelopeInput(
            issuedAtMs,
            expiresAtMs,
            actionJson,
        )
        return JSONObject()
            .put("message_type", BridgeExecuteProtocol.MESSAGE_TYPE_EXECUTE)
            .put("issued_at_ms", issuedAtMs)
            .put("expires_at_ms", expiresAtMs)
            .put(
                "auth",
                JSONObject().put(
                    "hmac_sha256",
                    BridgeExecuteProtocol.hmacSha256Hex(authToken, canonical),
                ),
            )
            .put("action", actionJson)
    }

    private companion object {
        const val AUTH_TOKEN = "test-token-123"
    }
}
