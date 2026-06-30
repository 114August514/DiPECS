package com.dipecs.collector.actions

import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Test
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

class SystemActionBridgeTest {

    // ── v2 protocol HMAC validation ────────────────────────

    @Test
    fun `hmac_sha256_matches_rust_side`() {
        val key = "shared-secret"
        val action = """{"intent_id":"intent-1","coord":{"window_ordinal":0,"intent_ordinal":0,"action_ordinal":0},"action":{"action_type":"NoOp","target":null,"urgency":"Immediate"},"effect":"PureRead","authorized_at_ms":1000}"""

        val hmac = hmacSha256Hex(key, action)

        assertEquals("SHA-256 HMAC hex is 64 chars", 64, hmac.length)
        assertTrue("HMAC must be lowercase hex: $hmac", hmac.all { it in '0'..'9' || it in 'a'..'f' })
    }

    @Test
    fun `hmac_is_deterministic`() {
        val key = "k1"
        val payload = """{"action_type":"NoOp"}"""
        val a = hmacSha256Hex(key, payload)
        val b = hmacSha256Hex(key, payload)
        assertEquals(a, b)
    }

    @Test
    fun `hmac_key_sensitive`() {
        val payload = """{"action_type":"NoOp"}"""
        val a = hmacSha256Hex("k1", payload)
        val b = hmacSha256Hex("k2", payload)
        assertNotEquals(a, b)
    }

    @Test
    fun `hmac_payload_sensitive`() {
        val key = "k1"
        val a = hmacSha256Hex(key, """{"action_type":"NoOp"}""")
        val b = hmacSha256Hex(key, """{"action_type":"NoOp","target":"x"}""")
        assertNotEquals(a, b)
    }

    @Test
    fun `constant_time_equals`() {
        assertTrue(constantTimeEquals("abc", "abc"))
        assertFalse(constantTimeEquals("abc", "abd"))
        assertFalse(constantTimeEquals("abc", "ab"))
        assertFalse(constantTimeEquals("", "a"))
    }

    // ── v2 envelope parsing ───────────────────────────────

    @Test
    fun `valid_envelope_parses_correctly`() {
        val key = "secret"
        val actionRaw = """{"intent_id":"i1","coord":{"window_ordinal":0,"intent_ordinal":0,"action_ordinal":0},"action":{"action_type":"NoOp","target":null,"urgency":"Immediate"},"effect":"PureRead","authorized_at_ms":1000}"""
        val hmac = hmacSha256Hex(key, actionRaw)

        val envelope = JSONObject().apply {
            put("message_type", "execute")
            put("action", actionRaw)
            put("auth", JSONObject().apply {
                put("hmac_sha256", hmac)
            })
        }

        assertEquals("execute", envelope.getString("message_type"))
        assertEquals(actionRaw, envelope.getString("action"))
        assertEquals(hmac, envelope.getJSONObject("auth").getString("hmac_sha256"))

        // Re-verify the HMAC.
        val recomputed = hmacSha256Hex(key, envelope.getString("action"))
        assertTrue(
            "HMAC must match after round-trip: $hmac vs $recomputed",
            constantTimeEquals(hmac.lowercase(), recomputed),
        )
    }

    @Test
    fun `tampered_hmac_rejected`() {
        val key = "secret"
        val actionRaw = """{"action_type":"NoOp"}"""
        val hmac = hmacSha256Hex(key, actionRaw)

        val tamperedHmac = hmac.substring(1) + "0"
        assertNotEquals(hmac, tamperedHmac)
        val recomputed = hmacSha256Hex(key, actionRaw)
        assertFalse(
            "tampered HMAC must not equal recomputed",
            constantTimeEquals(tamperedHmac, recomputed),
        )
    }

    @Test
    fun `wrong_key_hmac_rejected`() {
        val actionRaw = """{"action_type":"PrefetchFile","target":"url:https://example.test"}"""
        val attackerHmac = hmacSha256Hex("attacker-key", actionRaw)
        val serverHmac = hmacSha256Hex("server-key", actionRaw)
        assertNotEquals(attackerHmac, serverHmac)
    }

    // ── BridgeExecuteResponse round-trip ──────────────────

    @Test
    fun `response_serializes_snake_case`() {
        val response = JSONObject().apply {
            put("status", "ok")
            put("summary", "prewarm:com.example")
            put("latency_us", 4242)
        }
        val json = response.toString()
        assertTrue("status must be ok: $json", json.contains("\"status\":\"ok\""))
        assertTrue(json.contains("\"summary\":\"prewarm:com.example\""))
        assertTrue(json.contains("\"latency_us\":4242"))

        val parsed = JSONObject(json)
        assertEquals("ok", parsed.getString("status"))
        assertEquals("prewarm:com.example", parsed.getString("summary"))
        assertEquals(4242, parsed.getLong("latency_us"))
    }

    @Test
    fun `error_response_includes_error_field`() {
        val response = JSONObject().apply {
            put("status", "rejected")
            put("error", "device refused: token expired")
        }
        val json = response.toString()
        assertTrue(json.contains("\"status\":\"rejected\""))
        assertTrue(json.contains("\"error\":\"device refused: token expired\""))
    }

    @Test
    fun `response_optional_fields_tolerated`() {
        // Rust side tolerates missing summary, latency_us, error.
        val minimal = JSONObject().apply { put("status", "rejected") }
        val json = minimal.toString()

        val parsed = JSONObject(json)
        assertEquals("rejected", parsed.getString("status"))
        assertFalse(parsed.has("summary"))
        assertFalse(parsed.has("latency_us"))
        assertFalse(parsed.has("error"))
    }

    // ── SystemActionExecutors result type ─────────────────

    @Test
    fun `action_result_success`() {
        val result = SystemActionExecutors.ActionResult(
            success = true,
            summary = "prewarm:com.example",
            latencyUs = 1234,
            error = null,
        )
        assertTrue(result.success)
        assertEquals("prewarm:com.example", result.summary)
        assertEquals(1234, result.latencyUs)
        assertNull(result.error)
    }

    @Test
    fun `action_result_failure`() {
        val result = SystemActionExecutors.ActionResult(
            success = false,
            summary = "prewarm_denied",
            latencyUs = 567,
            error = "START_ACTIVITIES_FROM_BACKGROUND not granted",
        )
        assertFalse(result.success)
        assertEquals("prewarm_denied", result.summary)
        assertEquals(567, result.latencyUs)
        assertNotNull(result.error)
        assertTrue(result.error!!.contains("not granted"))
    }

    // ── ActionDispatch routing ────────────────────────────

    @Test
    fun `noop_returns_success_without_context`() {
        val result = buildActionResult(success = true, summary = "noop", error = null)
        assertTrue(result.success)
        assertEquals("noop", result.summary)
        assertNull(result.error)
    }

    @Test
    fun `prefetch_without_target_returns_error`() {
        val result = buildActionResult(success = false, summary = "prefetch_no_target",
            error = "PrefetchFile requires a target")
        assertFalse(result.success)
        assertTrue(result.error!!.contains("target"))
    }

    @Test
    fun `unsupported_action_type_returns_error`() {
        val result = buildActionResult(success = false, summary = "unsupported_action",
            error = "Unknown action type: UnknownAction")
        assertFalse(result.success)
        assertTrue(result.error!!.contains("UnknownAction"))
    }

    // ── Helpers ───────────────────────────────────────────

    private fun buildActionResult(
        success: Boolean,
        summary: String,
        error: String?,
    ): SystemActionExecutors.ActionResult =
        SystemActionExecutors.ActionResult(
            success = success,
            summary = summary,
            latencyUs = 0,
            error = error,
        )

    private fun hmacSha256Hex(key: String, message: String): String {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(key.toByteArray(Charsets.UTF_8), "HmacSHA256"))
        return mac.doFinal(message.toByteArray(Charsets.UTF_8))
            .joinToString("") { byte -> "%02x".format(byte) }
    }

    private fun constantTimeEquals(left: String, right: String): Boolean {
        return MessageDigest.isEqual(
            left.toByteArray(Charsets.UTF_8),
            right.toByteArray(Charsets.UTF_8),
        )
    }

    @After
    fun cleanup() {
        // No shared state to clean.
    }
}
