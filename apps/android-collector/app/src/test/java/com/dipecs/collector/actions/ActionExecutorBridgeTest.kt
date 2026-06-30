package com.dipecs.collector.actions

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ActionExecutorBridgeTest {
    @Test
    fun parseAuthorizedActionAcceptsActionTypeAndTarget() {
        val parsed = ActionExecutorBridge.parseAuthorizedAction(
            JSONObject()
                .put(
                    "action",
                    JSONObject()
                        .put("action_type", ActionExecutorBridge.ACTION_TYPE_PREFETCH_FILE)
                        .put("target", "url:https://example.test/feed.json"),
                ),
        )

        assertEquals(ActionExecutorBridge.ACTION_TYPE_PREFETCH_FILE, parsed?.actionType)
        assertEquals("url:https://example.test/feed.json", parsed?.target)
    }

    @Test
    fun parseAuthorizedActionRejectsMissingActionType() {
        val parsed = ActionExecutorBridge.parseAuthorizedAction(
            JSONObject().put("action", JSONObject().put("target", "own:resources")),
        )

        assertNull(parsed)
    }
}
