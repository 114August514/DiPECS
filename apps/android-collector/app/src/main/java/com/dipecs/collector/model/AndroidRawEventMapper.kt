package com.dipecs.collector.model

import org.json.JSONObject

object AndroidRawEventMapper {
    fun appTransition(
        timestampMs: Long,
        packageName: String,
        activityClass: String?,
        transition: String,
    ): JSONObject = tagged(
        "AppTransition",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("package_name", packageName)
            .put("activity_class", activityClass ?: JSONObject.NULL)
            .put("transition", transition),
    )

    fun notificationPosted(
        timestampMs: Long,
        packageName: String,
        category: String?,
        channelId: String?,
        rawTitle: String?,
        rawText: String?,
        isOngoing: Boolean,
        groupKey: String?,
        hasPicture: Boolean,
    ): JSONObject = tagged(
        "NotificationPosted",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("package_name", packageName)
            .put("category", category ?: JSONObject.NULL)
            .put("channel_id", channelId ?: JSONObject.NULL)
            .put("raw_title", rawTitle.orEmpty())
            .put("raw_text", rawText.orEmpty())
            .put("is_ongoing", isOngoing)
            .put("group_key", groupKey ?: JSONObject.NULL)
            .put("has_picture", hasPicture),
    )

    fun notificationInteraction(
        timestampMs: Long,
        packageName: String,
        notificationKey: String,
        action: String,
    ): JSONObject = tagged(
        "NotificationInteraction",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("package_name", packageName)
            .put("notification_key", notificationKey)
            .put("action", action),
    )

    fun screenState(timestampMs: Long, state: String): JSONObject = tagged(
        "ScreenState",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("state", state),
    )

    fun systemState(timestampMs: Long, context: DeviceContext): JSONObject = tagged(
        "SystemState",
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("battery_pct", context.batteryPercent ?: JSONObject.NULL)
            .put("is_charging", context.isCharging ?: false)
            .put("network", rustNetwork(context.networkType))
            .put("ringer_mode", rustRingerMode(context.ringerMode))
            .put("location_type", "Unknown")
            .put("headphone_connected", false)
            .put("bluetooth_connected", false),
    )

    fun rawEventKind(rawEvent: JSONObject?): String? {
        rawEvent ?: return null
        val keys = rawEvent.keys()
        return if (keys.hasNext()) keys.next() else null
    }

    private fun tagged(kind: String, payload: JSONObject): JSONObject =
        JSONObject().put(kind, payload)

    private fun rustNetwork(networkType: String): String = when (networkType) {
        "wifi", "ethernet", "bluetooth", "vpn" -> "Wifi"
        "cellular" -> "Cellular"
        "offline" -> "Offline"
        else -> "Unknown"
    }

    private fun rustRingerMode(ringerMode: String): String = when (ringerMode) {
        "normal" -> "Normal"
        "vibrate" -> "Vibrate"
        "silent" -> "Silent"
        else -> "Normal"
    }
}
