@file:Suppress("DEPRECATION")

package com.dipecs.collector.services

import android.app.Notification
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import com.dipecs.collector.collectors.DeviceContextCollector
import com.dipecs.collector.model.AndroidRawEventMapper
import com.dipecs.collector.model.CollectorEvent
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

class NotificationCollectorService : NotificationListenerService() {
    override fun onListenerConnected() {
        EventRepository.recordInternal(this, "notification_listener_connected", "Notification listener connected")
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) {
        if (!CollectorPreferences.isNotificationEnabled(this)) {
            return
        }
        EventRepository.record(this, notificationEvent("notification_posted", sbn))
    }

    override fun onNotificationRemoved(sbn: StatusBarNotification) {
        if (!CollectorPreferences.isNotificationEnabled(this)) {
            return
        }
        EventRepository.record(this, notificationRemovedEvent(sbn, reason = null))
    }

    override fun onNotificationRemoved(
        sbn: StatusBarNotification,
        _rankingMap: NotificationListenerService.RankingMap,
        reason: Int,
    ) {
        if (!CollectorPreferences.isNotificationEnabled(this)) {
            return
        }
        EventRepository.record(this, notificationRemovedEvent(sbn, reason))
    }

    private fun notificationEvent(eventType: String, sbn: StatusBarNotification): CollectorEvent {
        val extras = sbn.notification.extras
        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString()
        val text = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString()
        val subText = extras.getCharSequence(Notification.EXTRA_SUB_TEXT)?.toString()
        val bigText = extras.getCharSequence(Notification.EXTRA_BIG_TEXT)?.toString()
        val combinedText = listOfNotNull(text, bigText, subText).joinToString(" | ").ifBlank { null }
        val isOngoing = (sbn.notification.flags and Notification.FLAG_ONGOING_EVENT) != 0
        val hasPicture = extras.containsKey(Notification.EXTRA_PICTURE)
        val rawEvent = AndroidRawEventMapper.notificationPosted(
            timestampMs = sbn.postTime,
            packageName = sbn.packageName,
            category = sbn.notification.category,
            channelId = sbn.notification.channelId,
            rawTitle = title,
            rawText = combinedText,
            isOngoing = isOngoing,
            groupKey = sbn.groupKey,
            hasPicture = hasPicture,
        )

        return CollectorEvent(
            timestampMs = if (eventType == "notification_posted") sbn.postTime else System.currentTimeMillis(),
            source = "notification_listener",
            eventType = eventType,
            packageName = sbn.packageName,
            className = sbn.notification.category,
            text = combinedText,
            action = eventType,
            deviceContext = DeviceContextCollector.snapshot(this),
            rawEvent = rawEvent,
            rawPayload = JSONObject()
                .put("id", sbn.id)
                .put("tag", sbn.tag)
                .put("key", sbn.key)
                .put("category", sbn.notification.category)
                .put("channelId", sbn.notification.channelId)
                .put("priority", sbn.notification.priority)
                .put("isOngoing", isOngoing)
                .put("hasPicture", hasPicture)
                .put("foregroundPackage", CollectorPreferences.foregroundPackage(this)),
        )
    }

    private fun notificationRemovedEvent(sbn: StatusBarNotification, reason: Int?): CollectorEvent {
        val now = System.currentTimeMillis()
        val action = notificationActionForReason(reason)
        return CollectorEvent(
            timestampMs = now,
            source = "notification_listener",
            eventType = "notification_removed",
            packageName = sbn.packageName,
            className = sbn.notification.category,
            action = action,
            deviceContext = DeviceContextCollector.snapshot(this),
            rawEvent = AndroidRawEventMapper.notificationInteraction(
                timestampMs = now,
                packageName = sbn.packageName,
                notificationKey = sbn.key,
                action = action,
            ),
            rawPayload = JSONObject()
                .put("id", sbn.id)
                .put("tag", sbn.tag)
                .put("key", sbn.key)
                .put("category", sbn.notification.category)
                .put("removalReason", reason ?: JSONObject.NULL)
                .put("foregroundPackage", CollectorPreferences.foregroundPackage(this)),
        )
    }

    private fun notificationActionForReason(reason: Int?): String = when (reason) {
        REASON_CLICK -> "Tapped"
        REASON_CANCEL,
        REASON_CANCEL_ALL,
        REASON_LISTENER_CANCEL,
        REASON_LISTENER_CANCEL_ALL -> "Dismissed"
        else -> "Cancelled"
    }
}
