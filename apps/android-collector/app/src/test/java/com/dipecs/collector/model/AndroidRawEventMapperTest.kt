package com.dipecs.collector.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRawEventMapperTest {
    @Test
    fun appTransitionForegroundMatchesRustRawEventJson() {
        val rawEvent = AndroidRawEventMapper.appTransition(
            timestampMs = 1000L,
            packageName = "com.android.chrome",
            activityClass = "MainActivity",
            transition = "Foreground",
        )

        val event = rawEvent.getJSONObject("AppTransition")
        assertEquals(1000L, event.getLong("timestamp_ms"))
        assertEquals("com.android.chrome", event.getString("package_name"))
        assertEquals("MainActivity", event.getString("activity_class"))
        assertEquals("Foreground", event.getString("transition"))
        assertEquals("AppTransition", AndroidRawEventMapper.rawEventKind(rawEvent))
    }

    @Test
    fun notificationPostedMatchesRustRawEventJson() {
        val rawEvent = AndroidRawEventMapper.notificationPosted(
            timestampMs = 2000L,
            packageName = "com.ss.android.lark",
            category = "msg",
            channelId = "lark_im_message",
            rawTitle = "张三",
            rawText = "发来一个文件",
            isOngoing = false,
            groupKey = "group-1",
            hasPicture = true,
        )

        val event = rawEvent.getJSONObject("NotificationPosted")
        assertEquals(2000L, event.getLong("timestamp_ms"))
        assertEquals("com.ss.android.lark", event.getString("package_name"))
        assertEquals("msg", event.getString("category"))
        assertEquals("lark_im_message", event.getString("channel_id"))
        assertEquals("张三", event.getString("raw_title"))
        assertEquals("发来一个文件", event.getString("raw_text"))
        assertFalse(event.getBoolean("is_ongoing"))
        assertEquals("group-1", event.getString("group_key"))
        assertTrue(event.getBoolean("has_picture"))
    }

    @Test
    fun notificationInteractionMatchesRustRawEventJson() {
        val rawEvent = AndroidRawEventMapper.notificationInteraction(
            timestampMs = 2500L,
            packageName = "com.ss.android.lark",
            notificationKey = "0|com.ss.android.lark|42|null|10086",
            action = "Tapped",
        )

        val event = rawEvent.getJSONObject("NotificationInteraction")
        assertEquals(2500L, event.getLong("timestamp_ms"))
        assertEquals("com.ss.android.lark", event.getString("package_name"))
        assertEquals("0|com.ss.android.lark|42|null|10086", event.getString("notification_key"))
        assertEquals("Tapped", event.getString("action"))
    }

    @Test
    fun systemStateMapsDeviceContextToRustRawEventJson() {
        val rawEvent = AndroidRawEventMapper.systemState(
            timestampMs = 3000L,
            context = DeviceContext(
                timezone = "Asia/Shanghai",
                batteryPercent = 88,
                isCharging = true,
                networkType = "wifi",
                isScreenOn = true,
                ringerMode = "vibrate",
                doNotDisturbMode = null,
            ),
        )

        val event = rawEvent.getJSONObject("SystemState")
        assertEquals(3000L, event.getLong("timestamp_ms"))
        assertEquals(88, event.getInt("battery_pct"))
        assertTrue(event.getBoolean("is_charging"))
        assertEquals("Wifi", event.getString("network"))
        assertEquals("Vibrate", event.getString("ringer_mode"))
        assertEquals("Unknown", event.getString("location_type"))
        assertFalse(event.getBoolean("headphone_connected"))
        assertFalse(event.getBoolean("bluetooth_connected"))
    }
}
