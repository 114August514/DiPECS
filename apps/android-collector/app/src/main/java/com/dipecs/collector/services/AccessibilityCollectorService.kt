package com.dipecs.collector.services

import android.accessibilityservice.AccessibilityService
import android.view.accessibility.AccessibilityEvent
import com.dipecs.collector.collectors.DeviceContextCollector
import com.dipecs.collector.model.CollectorEvent
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONArray
import org.json.JSONObject

class AccessibilityCollectorService : AccessibilityService() {
    override fun onServiceConnected() {
        EventRepository.recordInternal(this, "accessibility_connected", "Accessibility service connected")
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event == null) {
            return
        }
        if (!CollectorPreferences.isAccessibilityEnabled(this)) {
            return
        }

        val packageName = event.packageName?.toString()
        val className = event.className?.toString()
        if (event.eventType == AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED ||
            event.eventType == AccessibilityEvent.TYPE_WINDOWS_CHANGED
        ) {
            CollectorPreferences.setForeground(this, packageName, className)
        }

        val sourceNode = runCatching { event.source }.getOrNull()
        val eventText = event.text
            ?.mapNotNull { it?.toString() }
            ?.joinToString(" | ")
            ?.ifBlank { null }

        EventRepository.record(
            this,
            CollectorEvent(
                timestampMs = event.eventTime.takeIf { it > 0 } ?: System.currentTimeMillis(),
                source = "accessibility",
                eventType = accessibilityEventName(event.eventType),
                packageName = packageName,
                className = className,
                windowTitle = event.contentDescription?.toString(),
                text = eventText,
                action = accessibilityEventName(event.eventType),
                deviceContext = DeviceContextCollector.snapshot(this),
                rawPayload = JSONObject()
                    .put("eventType", event.eventType)
                    .put("eventTypeName", AccessibilityEvent.eventTypeToString(event.eventType))
                    .put("contentChangeTypes", event.contentChangeTypes)
                    .put("movementGranularity", event.movementGranularity)
                    .put("itemCount", event.itemCount)
                    .put("currentItemIndex", event.currentItemIndex)
                    .put("fromIndex", event.fromIndex)
                    .put("toIndex", event.toIndex)
                    .put("scrollX", event.scrollX)
                    .put("scrollY", event.scrollY)
                    .put("viewIdResourceName", sourceNode?.viewIdResourceName)
                    .put("sourceText", sourceNode?.text?.toString())
                    .put("sourceClassName", sourceNode?.className?.toString())
                    .put("sourceContentDescription", sourceNode?.contentDescription?.toString())
                    .put("textItems", JSONArray(event.text ?: emptyList<CharSequence>())),
            ),
        )
    }

    override fun onInterrupt() {
        EventRepository.recordInternal(this, "accessibility_interrupted", "Accessibility service interrupted")
    }

    private fun accessibilityEventName(eventType: Int): String = when (eventType) {
        AccessibilityEvent.TYPE_VIEW_CLICKED -> "view_clicked"
        AccessibilityEvent.TYPE_VIEW_FOCUSED -> "view_focused"
        AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED -> "view_text_changed"
        AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> "window_state_changed"
        AccessibilityEvent.TYPE_WINDOWS_CHANGED -> "windows_changed"
        AccessibilityEvent.TYPE_VIEW_SELECTED -> "view_selected"
        AccessibilityEvent.TYPE_NOTIFICATION_STATE_CHANGED -> "notification_state_changed"
        else -> "accessibility_event_$eventType"
    }
}
