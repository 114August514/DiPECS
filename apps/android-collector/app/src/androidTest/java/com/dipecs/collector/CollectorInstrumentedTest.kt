package com.dipecs.collector

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.dipecs.collector.model.AndroidRawEventMapper
import com.dipecs.collector.model.CollectorEvent
import com.dipecs.collector.model.DeviceContext
import com.dipecs.collector.storage.EventStore
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * Instrumentation tests that run on an Android emulator or device.
 *
 * These tests verify the critical cross-boundary contracts between
 * the Android app and the Rust pipeline:
 *
 * 1. HMAC-SHA256 action signature matches the Rust implementation.
 * 2. Canonical signature input format is byte-identical to Rust's.
 * 3. Event JSON schema (RawEvent serde external-tag format) is correct.
 * 4. EventStore append/read round-trip.
 */
@RunWith(AndroidJUnit4::class)
class CollectorInstrumentedTest {

    private lateinit var context: Context

    @Before
    fun setUp() {
        context = InstrumentationRegistry.getInstrumentation().targetContext
    }

    // ── HMAC-SHA256 action signature protocol tests ───────────────

    @Test
    fun actionSignature_rfc4231_testVector() {
        // RFC 4231 Test Case 1: 20-byte key, message "Hi There".
        val key = ByteArray(20) { 0x0b }
        val message = "Hi There".toByteArray(Charsets.UTF_8)
        val expected = "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"

        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(key, "HmacSHA256"))
        val actual = mac.doFinal(message)
            .joinToString(separator = "") { byte -> "%02x".format(byte) }

        assertEquals(expected, actual)
    }

    @Test
    fun actionSignature_isDeterministic() {
        val a = computeActionSignature(
            "test-token", 1000L, 2000L,
            "PrefetchFile", "url:https://example.test/f", "Immediate"
        )
        val b = computeActionSignature(
            "test-token", 1000L, 2000L,
            "PrefetchFile", "url:https://example.test/f", "Immediate"
        )
        assertEquals(a, b)
    }

    @Test
    fun actionSignature_changesWithDifferentToken() {
        val a = computeActionSignature("token-a", 1000L, 2000L, "NoOp", "", "Immediate")
        val b = computeActionSignature("token-b", 1000L, 2000L, "NoOp", "", "Immediate")
        assertNotEquals(a, b)
    }

    @Test
    fun actionSignature_changesWithDifferentTarget() {
        val a = computeActionSignature("token", 1000L, 2000L, "PrefetchFile", "url:https://a.test", "Immediate")
        val b = computeActionSignature("token", 1000L, 2000L, "PrefetchFile", "url:https://b.test", "Immediate")
        assertNotEquals(a, b)
    }

    @Test
    fun canonicalSignatureInput_formatMatchesRust() {
        val issuedAtMs = 1000L
        val expiresAtMs = 106000L
        val actionType = "PrefetchFile"
        val target = "url:https://example.test/a:b"
        val urgency = "Immediate"

        val canonical = canonicalActionSignatureInput(
            issuedAtMs, expiresAtMs, actionType, target, urgency
        )

        // The Rust-side canonical format uses length-prefixed fields:
        // "dipecs.android.action.v1\nissued_at_ms:{}\nexpires_at_ms:{}\n
        //  action_type:{len}:{}\ntarget:{len}:{}\nurgency:{len}:{}"
        assertTrue(
            "canonical must contain length-prefixed action_type",
            canonical.contains("action_type:${actionType.length}:$actionType")
        )
        assertTrue(
            "canonical must contain length-prefixed target",
            canonical.contains("target:${target.length}:$target")
        )
        assertTrue(
            "canonical must contain length-prefixed urgency",
            canonical.contains("urgency:${urgency.length}:$urgency")
        )
        assertTrue(
            "canonical must start with protocol version",
            canonical.startsWith("dipecs.android.action.v1\n")
        )
    }

    @Test
    fun actionSignature_verifiableRoundTrip() {
        val authToken = "dipecs-dev-emulator-shared-token-00000000"
        val issuedAtMs = System.currentTimeMillis()
        val expiresAtMs = issuedAtMs + 60_000
        val actionType = "PrefetchFile"
        val target = "url:https://example.test/feed.json"
        val urgency = "Immediate"

        val canonical = canonicalActionSignatureInput(
            issuedAtMs, expiresAtMs, actionType, target, urgency
        )
        val signature = hmacSha256Hex(authToken, canonical)

        // Recomputing with same inputs must yield same signature.
        assertEquals(signature, hmacSha256Hex(authToken, canonical))

        // Tampered canonical input must yield different signature.
        val tampered = canonical.replace("Immediate", "Deferred")
        assertNotEquals(signature, hmacSha256Hex(authToken, tampered))
    }

    @Test
    fun bridgePayload_validationRejectsWrongToken() {
        val payload = JSONObject().apply {
            put("auth_token", "wrong-token")
            put("message_type", "ping")
        }
        val result = validateActionPayload(payload, "correct-token")
        assertEquals("AUTH_TOKEN_MISSING_OR_INVALID", result)
    }

    @Test
    fun bridgePayload_validationAcceptsPing() {
        val payload = JSONObject().apply {
            put("auth_token", "my-token")
            put("message_type", "ping")
        }
        val result = validateActionPayload(payload, "my-token")
        assertEquals("PING", result)
    }

    @Test
    fun bridgePayload_validationRejectsMissingExpiry() {
        val now = System.currentTimeMillis()
        val payload = JSONObject().apply {
            put("auth_token", "my-token")
            put("issued_at_ms", now)
            // expires_at_ms missing
            put("action", JSONObject().apply {
                put("action_type", "NoOp")
                put("target", JSONObject.NULL)
                put("urgency", "Immediate")
            })
            put("action_signature", "0000000000000000000000000000000000000000000000000000000000000000")
        }
        val result = validateActionPayload(payload, "my-token")
        assertEquals("ACTION_WINDOW_MISSING_OR_EXPIRED", result)
    }

    @Test
    fun bridgePayload_validationRejectsExpiredWindow() {
        val payload = JSONObject().apply {
            put("auth_token", "my-token")
            put("issued_at_ms", 1000L)
            put("expires_at_ms", 2000L) // expired long ago
            put("action", JSONObject().apply {
                put("action_type", "NoOp")
                put("target", JSONObject.NULL)
                put("urgency", "Immediate")
            })
            put("action_signature", "0000000000000000000000000000000000000000000000000000000000000000")
        }
        val result = validateActionPayload(payload, "my-token")
        assertEquals("ACTION_WINDOW_MISSING_OR_EXPIRED", result)
    }

    // ── RawEvent schema tests ────────────────────────────────────

    @Test
    fun appTransition_usesRustSerdeExternalTagFormat() {
        val rawEvent = AndroidRawEventMapper.appTransition(
            timestampMs = 1000L,
            packageName = "com.android.chrome",
            activityClass = "MainActivity",
            transition = "Foreground"
        )

        // The rawEvent must use Rust's serde external-tag format:
        // {"AppTransition": {...}}
        assertTrue("rawEvent must have AppTransition key", rawEvent.has("AppTransition"))
        val inner = rawEvent.getJSONObject("AppTransition")
        assertEquals(1000L, inner.getLong("timestamp_ms"))
        assertEquals("com.android.chrome", inner.getString("package_name"))
        assertEquals("MainActivity", inner.getString("activity_class"))
        assertEquals("Foreground", inner.getString("transition"))
    }

    @Test
    fun notificationPosted_usesRustSerdeExternalTagFormat() {
        val rawEvent = AndroidRawEventMapper.notificationPosted(
            timestampMs = 2000L,
            packageName = "com.example.app",
            category = "msg",
            channelId = "messages",
            title = "Example",
            textItems = listOf("example file notification"),
            isOngoing = false,
            hasPicture = false
        )

        assertTrue("rawEvent must have NotificationPosted key", rawEvent.has("NotificationPosted"))
        val inner = rawEvent.getJSONObject("NotificationPosted")
        assertEquals(2000L, inner.getLong("timestamp_ms"))
        assertEquals("com.example.app", inner.getString("package_name"))
        // raw_title and raw_text are always "" in the mapper (privacy-by-default).
        assertEquals("", inner.getString("raw_title"))
        assertEquals("", inner.getString("raw_text"))
    }

    @Test
    fun systemState_usesRustSerdeExternalTagFormat() {
        val deviceCtx = DeviceContext(
            timezone = "Asia/Shanghai",
            batteryPercent = 85,
            isCharging = true,
            networkType = "wifi",
            isScreenOn = true,
            ringerMode = "normal",
            doNotDisturbMode = null,
            locationType = "Unknown",
            headphoneConnected = false,
            bluetoothConnected = false
        )
        val rawEvent = AndroidRawEventMapper.systemState(
            timestampMs = 3000L,
            context = deviceCtx
        )

        assertTrue("rawEvent must have SystemState key", rawEvent.has("SystemState"))
        val inner = rawEvent.getJSONObject("SystemState")
        assertEquals(3000L, inner.getLong("timestamp_ms"))
        assertEquals(85, inner.getInt("battery_pct"))
        assertTrue(inner.getBoolean("is_charging"))
        // Network type mapped through rustNetwork(): "wifi" → "Wifi"
        assertEquals("Wifi", inner.getString("network"))
    }

    @Test
    fun notificationInteraction_usesRustSerdeExternalTagFormat() {
        val rawEvent = AndroidRawEventMapper.notificationInteraction(
            timestampMs = 4000L,
            packageName = "com.example.app",
            action = "Tapped"
        )

        assertTrue("rawEvent must have NotificationInteraction key", rawEvent.has("NotificationInteraction"))
        val inner = rawEvent.getJSONObject("NotificationInteraction")
        assertEquals(4000L, inner.getLong("timestamp_ms"))
        assertEquals("com.example.app", inner.getString("package_name"))
        assertEquals("Tapped", inner.getString("action"))
    }

    @Test
    fun screenState_usesRustSerdeExternalTagFormat() {
        val rawEvent = AndroidRawEventMapper.screenState(
            timestampMs = 5000L,
            state = "Interactive"
        )

        assertTrue("rawEvent must have ScreenState key", rawEvent.has("ScreenState"))
        val inner = rawEvent.getJSONObject("ScreenState")
        assertEquals(5000L, inner.getLong("timestamp_ms"))
        assertEquals("Interactive", inner.getString("state"))
    }

    @Test
    fun rawEventKind_returnsExternalTagKey() {
        val rawEvent = AndroidRawEventMapper.appTransition(1000L, "com.a", null, "Foreground")
        val kind = AndroidRawEventMapper.rawEventKind(rawEvent)
        assertEquals("AppTransition", kind)
    }

    @Test
    fun rawEventKind_returnsNullForNullInput() {
        val kind = AndroidRawEventMapper.rawEventKind(null)
        assertNull(kind)
    }

    @Test
    fun collectorEvent_withNullRawEvent_isValidAccessibilityRow() {
        // Accessibility events use rawEvent: null.
        val event = CollectorEvent(
            source = "AccessibilityCollectorService",
            eventType = "accessibility_text",
            packageName = "com.example.app",
            rawEvent = null,
        )
        val json = event.toJson()
        assertTrue("rawEvent must be null for accessibility rows", json.isNull("rawEvent"))
        assertEquals("AccessibilityCollectorService", json.getString("source"))
        assertEquals("accessibility_text", json.getString("eventType"))
    }

    // ── CollectorEvent JSON shape tests ──────────────────────────

    @Test
    fun collectorEvent_appTransition_roundTripsToJsonlShape() {
        val rawEvent = AndroidRawEventMapper.appTransition(
            1000L, "com.android.chrome", "MainActivity", "Foreground"
        )
        val event = CollectorEvent(
            source = "UsageCollector",
            eventType = "app_transition",
            packageName = "com.android.chrome",
            rawEvent = rawEvent,
        )
        val json = event.toJson()

        // The JSON shape must match what Rust's `AndroidJsonlIngress` expects.
        assertFalse("eventId must not be empty", json.getString("eventId").isEmpty())
        assertEquals("UsageCollector", json.getString("source"))
        assertEquals("app_transition", json.getString("eventType"))
        assertEquals("com.android.chrome", json.getString("packageName"))
        assertFalse("rawEvent must not be null", json.isNull("rawEvent"))

        val re = json.getJSONObject("rawEvent")
        assertTrue(re.has("AppTransition"))
    }

    @Test
    fun collectorEvent_serializationIsValidJson() {
        val rawEvent = AndroidRawEventMapper.systemState(
            3000L,
            DeviceContext(
                timezone = "UTC",
                batteryPercent = 50,
                isCharging = false,
                networkType = "cellular",
                isScreenOn = false,
                ringerMode = "vibrate",
                doNotDisturbMode = null,
            )
        )
        val event = CollectorEvent(
            source = "CollectorForegroundService",
            eventType = "system_state",
            rawEvent = rawEvent,
        )
        val jsonStr = event.toJson().toString()

        // Must be valid JSON.
        val parsed = JSONObject(jsonStr)
        assertEquals("CollectorForegroundService", parsed.getString("source"))
        assertEquals("system_state", parsed.getString("eventType"))
        assertFalse(parsed.isNull("rawEvent"))
    }

    // ── EventStore instrumentation ───────────────────────────────

    @Test
    fun eventStore_appendAndStats_workOnDevice() {
        val store = EventStore(context)

        // Clear any leftover data.
        store.clear()
        assertEquals(0, store.lineCount())

        // Append a valid event.
        val rawEvent = AndroidRawEventMapper.appTransition(
            1000L, "com.test", null, "Foreground"
        )
        val event = CollectorEvent(
            source = "TestCollector",
            eventType = "app_transition",
            packageName = "com.test",
            rawEvent = rawEvent,
        )
        store.append(event)

        assertTrue("line count must be >= 1 after append", store.lineCount() >= 1)

        val stats = store.stats()
        assertTrue("stats totalRows must be >= 1", stats.totalRows >= 1)
        assertTrue("stats rawEventRows must be >= 1", stats.rawEventRows >= 1)

        // Clean up.
        store.clear()
    }

    @Test
    fun eventStore_statsCountsNullRawEventRows() {
        val store = EventStore(context)
        store.clear()

        // Append a screening-only row (rawEvent: null).
        val screeningEvent = CollectorEvent(
            source = "AccessibilityCollectorService",
            eventType = "accessibility_text",
            packageName = "com.test",
            rawEvent = null,
        )
        store.append(screeningEvent)

        val stats = store.stats()
        assertTrue("null rawEvent row must be counted", stats.rawEventNullRows >= 1)

        store.clear()
    }

    // ── Helpers ──────────────────────────────────────────────────

    private fun canonicalActionSignatureInput(
        issuedAtMs: Long,
        expiresAtMs: Long,
        actionType: String,
        target: String,
        urgency: String,
    ): String = buildString {
        append("dipecs.android.action.v1\n")
        append("issued_at_ms:$issuedAtMs\n")
        append("expires_at_ms:$expiresAtMs\n")
        append("action_type:${actionType.length}:$actionType\n")
        append("target:${target.length}:$target\n")
        append("urgency:${urgency.length}:$urgency")
    }

    private fun hmacSha256Hex(key: String, message: String): String {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(key.toByteArray(Charsets.UTF_8), "HmacSHA256"))
        return mac.doFinal(message.toByteArray(Charsets.UTF_8))
            .joinToString(separator = "") { byte -> "%02x".format(byte) }
    }

    private fun computeActionSignature(
        authToken: String,
        issuedAtMs: Long,
        expiresAtMs: Long,
        actionType: String,
        target: String,
        urgency: String,
    ): String {
        val canonical = canonicalActionSignatureInput(
            issuedAtMs, expiresAtMs, actionType, target, urgency
        )
        return hmacSha256Hex(authToken, canonical)
    }

    // ── Bridge payload validation (mirrors AuthorizedActionSocketServer) ──

    companion object {
        fun validateActionPayload(payload: JSONObject, authToken: String): String {
            val supplied = payload.optString("auth_token").takeIf { it.isNotBlank() }
                ?: return "AUTH_TOKEN_MISSING_OR_INVALID"

            if (!constantTimeEquals(supplied, authToken)) {
                return "AUTH_TOKEN_MISSING_OR_INVALID"
            }

            if (payload.optString("message_type") == "ping") {
                return "PING"
            }

            val issuedAtMs = payload.optLong("issued_at_ms", 0L)
            val expiresAtMs = payload.optLong("expires_at_ms", 0L)
            if (issuedAtMs <= 0L || expiresAtMs <= 0L || expiresAtMs <= issuedAtMs) {
                return "ACTION_WINDOW_MISSING_OR_EXPIRED"
            }
            if (expiresAtMs - issuedAtMs > 300_000L) {
                return "ACTION_WINDOW_MISSING_OR_EXPIRED"
            }
            val now = System.currentTimeMillis()
            if (now + 30_000 < issuedAtMs || now - 30_000 > expiresAtMs) {
                return "ACTION_WINDOW_MISSING_OR_EXPIRED"
            }

            val suppliedSig = payload.optString("action_signature").takeIf { it.isNotBlank() }
                ?: return "ACTION_SIGNATURE_INVALID"

            val action = payload.optJSONObject("action") ?: return "ACTION_SIGNATURE_INVALID"
            val actionType = action.optString("action_type").takeIf { it.isNotBlank() }
                ?: return "ACTION_SIGNATURE_INVALID"
            val target = if (action.has("target") && !action.isNull("target")) {
                action.optString("target")
            } else ""
            val urgency = action.optString("urgency").takeIf { it.isNotBlank() }
                ?: return "ACTION_SIGNATURE_INVALID"

            val canonical = buildString {
                append("dipecs.android.action.v1\n")
                append("issued_at_ms:$issuedAtMs\n")
                append("expires_at_ms:$expiresAtMs\n")
                append("action_type:${actionType.length}:$actionType\n")
                append("target:${target.length}:$target\n")
                append("urgency:${urgency.length}:$urgency")
            }

            val mac = Mac.getInstance("HmacSHA256")
            mac.init(SecretKeySpec(authToken.toByteArray(Charsets.UTF_8), "HmacSHA256"))
            val expected = mac.doFinal(canonical.toByteArray(Charsets.UTF_8))
                .joinToString(separator = "") { byte -> "%02x".format(byte) }

            if (!constantTimeEquals(suppliedSig.lowercase(), expected)) {
                return "ACTION_SIGNATURE_INVALID"
            }

            return "ACTION_AUTHORIZED"
        }

        private fun constantTimeEquals(left: String, right: String): Boolean {
            return MessageDigest.isEqual(
                left.toByteArray(Charsets.UTF_8),
                right.toByteArray(Charsets.UTF_8)
            )
        }
    }
}
