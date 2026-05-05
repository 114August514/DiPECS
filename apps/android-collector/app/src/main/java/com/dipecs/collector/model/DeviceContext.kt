package com.dipecs.collector.model

import org.json.JSONObject

data class DeviceContext(
    val timezone: String,
    val batteryPercent: Int?,
    val isCharging: Boolean?,
    val networkType: String,
    val isScreenOn: Boolean,
    val ringerMode: String,
    val doNotDisturbMode: Int?,
) {
    fun toJson(): JSONObject = JSONObject()
        .put("timezone", timezone)
        .put("batteryPercent", batteryPercent ?: JSONObject.NULL)
        .put("isCharging", isCharging ?: JSONObject.NULL)
        .put("networkType", networkType)
        .put("isScreenOn", isScreenOn)
        .put("ringerMode", ringerMode)
        .put("doNotDisturbMode", doNotDisturbMode ?: JSONObject.NULL)
}
