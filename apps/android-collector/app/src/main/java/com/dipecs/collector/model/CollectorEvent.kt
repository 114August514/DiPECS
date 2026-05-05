package com.dipecs.collector.model

import org.json.JSONObject
import java.util.UUID

data class CollectorEvent(
    val eventId: String = UUID.randomUUID().toString(),
    val timestampMs: Long = System.currentTimeMillis(),
    val source: String,
    val eventType: String,
    val packageName: String? = null,
    val className: String? = null,
    val windowTitle: String? = null,
    val text: String? = null,
    val action: String? = null,
    val deviceContext: DeviceContext? = null,
    val rawEvent: JSONObject? = null,
    val rawPayload: JSONObject = JSONObject(),
) {
    fun toJson(): JSONObject = JSONObject()
        .put("eventId", eventId)
        .put("timestampMs", timestampMs)
        .put("source", source)
        .put("eventType", eventType)
        .put("packageName", packageName ?: JSONObject.NULL)
        .put("className", className ?: JSONObject.NULL)
        .put("windowTitle", windowTitle ?: JSONObject.NULL)
        .put("text", text ?: JSONObject.NULL)
        .put("action", action ?: JSONObject.NULL)
        .put("deviceContext", deviceContext?.toJson() ?: JSONObject.NULL)
        .put("rawEvent", rawEvent ?: JSONObject.NULL)
        .put("rawPayload", rawPayload)
}
