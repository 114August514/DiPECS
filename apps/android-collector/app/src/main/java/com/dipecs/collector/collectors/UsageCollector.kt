@file:Suppress("DEPRECATION")

package com.dipecs.collector.collectors

import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.Context
import android.os.Build
import com.dipecs.collector.model.AndroidRawEventMapper
import com.dipecs.collector.model.CollectorEvent
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

class UsageCollector(private val context: Context) {
    private val appContext = context.applicationContext

    fun collectSinceLastPoll() {
        if (!CollectorPreferences.isUsageEnabled(appContext)) {
            return
        }
        val usageStatsManager = appContext.getSystemService(UsageStatsManager::class.java) ?: return
        val now = System.currentTimeMillis()
        val start = CollectorPreferences.lastUsageQueryMs(appContext).coerceAtMost(now - 1)
        val usageEvents = runCatching { usageStatsManager.queryEvents(start, now) }.getOrNull() ?: return
        val event = UsageEvents.Event()

        while (usageEvents.hasNextEvent()) {
            usageEvents.getNextEvent(event)
            val eventType = usageEventName(event.eventType)
            if (!isInterestingUsageEvent(event.eventType)) {
                continue
            }

            if (isForegroundEvent(event.eventType)) {
                CollectorPreferences.setForeground(appContext, event.packageName, event.className)
            }

            val rawEvent = rawEventForUsageEvent(event)
            EventRepository.record(
                appContext,
                CollectorEvent(
                    timestampMs = event.timeStamp,
                    source = "usage_stats",
                    eventType = eventType,
                    packageName = event.packageName,
                    className = event.className,
                    action = eventType,
                    deviceContext = DeviceContextCollector.snapshot(appContext),
                    rawEvent = rawEvent,
                    rawPayload = JSONObject()
                        .put("usageEventType", event.eventType)
                        .put("configuration", event.configuration?.toString()),
                ),
            )
        }
        CollectorPreferences.setLastUsageQueryMs(appContext, now)
    }

    private fun isInterestingUsageEvent(eventType: Int): Boolean =
        isForegroundEvent(eventType) ||
            eventType == UsageEvents.Event.ACTIVITY_PAUSED ||
            eventType == UsageEvents.Event.ACTIVITY_STOPPED ||
            eventType == UsageEvents.Event.MOVE_TO_BACKGROUND ||
            eventType == UsageEvents.Event.CONFIGURATION_CHANGE ||
            eventType == UsageEvents.Event.SCREEN_INTERACTIVE ||
            eventType == UsageEvents.Event.SCREEN_NON_INTERACTIVE ||
            eventType == UsageEvents.Event.KEYGUARD_SHOWN ||
            eventType == UsageEvents.Event.KEYGUARD_HIDDEN

    private fun isForegroundEvent(eventType: Int): Boolean =
        eventType == UsageEvents.Event.MOVE_TO_FOREGROUND ||
            eventType == UsageEvents.Event.ACTIVITY_RESUMED

    private fun isBackgroundEvent(eventType: Int): Boolean =
        eventType == UsageEvents.Event.MOVE_TO_BACKGROUND ||
            eventType == UsageEvents.Event.ACTIVITY_PAUSED ||
            eventType == UsageEvents.Event.ACTIVITY_STOPPED

    private fun rawEventForUsageEvent(event: UsageEvents.Event): JSONObject? = when {
        isForegroundEvent(event.eventType) -> appTransitionRawEvent(event, "Foreground")
        isBackgroundEvent(event.eventType) -> appTransitionRawEvent(event, "Background")
        event.eventType == UsageEvents.Event.SCREEN_INTERACTIVE ->
            AndroidRawEventMapper.screenState(event.timeStamp, "Interactive")
        event.eventType == UsageEvents.Event.SCREEN_NON_INTERACTIVE ->
            AndroidRawEventMapper.screenState(event.timeStamp, "NonInteractive")
        event.eventType == UsageEvents.Event.KEYGUARD_SHOWN ->
            AndroidRawEventMapper.screenState(event.timeStamp, "KeyguardShown")
        event.eventType == UsageEvents.Event.KEYGUARD_HIDDEN ->
            AndroidRawEventMapper.screenState(event.timeStamp, "KeyguardHidden")
        else -> null
    }

    private fun appTransitionRawEvent(event: UsageEvents.Event, transition: String): JSONObject? {
        val packageName = event.packageName?.takeIf { it.isNotBlank() } ?: return null
        return AndroidRawEventMapper.appTransition(
            timestampMs = event.timeStamp,
            packageName = packageName,
            activityClass = event.className,
            transition = transition,
        )
    }

    private fun usageEventName(eventType: Int): String = when (eventType) {
        UsageEvents.Event.MOVE_TO_FOREGROUND -> "move_to_foreground"
        UsageEvents.Event.MOVE_TO_BACKGROUND -> "move_to_background"
        UsageEvents.Event.CONFIGURATION_CHANGE -> "configuration_change"
        UsageEvents.Event.SCREEN_INTERACTIVE -> "screen_interactive"
        UsageEvents.Event.SCREEN_NON_INTERACTIVE -> "screen_non_interactive"
        UsageEvents.Event.KEYGUARD_SHOWN -> "keyguard_shown"
        UsageEvents.Event.KEYGUARD_HIDDEN -> "keyguard_hidden"
        UsageEvents.Event.ACTIVITY_RESUMED -> "activity_resumed"
        UsageEvents.Event.ACTIVITY_PAUSED -> "activity_paused"
        UsageEvents.Event.ACTIVITY_STOPPED -> "activity_stopped"
        else -> if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            "usage_event_$eventType"
        } else {
            "usage_event_$eventType"
        }
    }
}
