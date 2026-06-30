package com.dipecs.collector.storage

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EventStoreTest {
    @Test
    fun sanitizeForTraceRedactsSensitiveFieldsWithoutBreakingRawEventSchema() {
        val original = JSONObject()
            .put("text", "private message")
            .put(
                "rawEvent",
                JSONObject()
                    .put(
                        "NotificationPosted",
                        JSONObject()
                            .put("timestamp_ms", 1L)
                            .put("package_name", "com.chat")
                            .put("raw_title", "Alice")
                            .put("raw_text", "secret")
                            .put("group_key", "conversation"),
                    ),
            )
            .put(
                "rawPayload",
                JSONObject()
                    .put("key", "0|com.chat|42|alice|1000")
                    .put("sourceText", "typed secret")
                    .put("target", "https://example.test/private")
                    .put("cachePath", "/data/user/0/com.dipecs/cache/private.json"),
            )

        val sanitized = EventStore.sanitizeForTrace(original)
        val event = sanitized
            .getJSONObject("rawEvent")
            .getJSONObject("NotificationPosted")

        assertTrue(sanitized.isNull("text"))
        assertEquals("", event.getString("raw_title"))
        assertEquals("", event.getString("raw_text"))
        assertTrue(event.isNull("group_key"))
        assertTrue(sanitized.getJSONObject("rawPayload").isNull("key"))
        assertTrue(sanitized.getJSONObject("rawPayload").isNull("sourceText"))
        assertTrue(sanitized.getJSONObject("rawPayload").isNull("target"))
        assertTrue(sanitized.getJSONObject("rawPayload").isNull("cachePath"))
    }
}
