package com.dipecs.collector.storage

import android.content.Context
import com.dipecs.collector.collectors.DeviceContextCollector
import com.dipecs.collector.model.CollectorEvent
import org.json.JSONObject

object EventRepository {
    fun record(context: Context, event: CollectorEvent) {
        EventStore(context).append(event)
    }

    fun recordInternal(
        context: Context,
        eventType: String,
        message: String,
        rawPayload: JSONObject = JSONObject(),
    ) {
        record(
            context,
            CollectorEvent(
                source = "internal",
                eventType = eventType,
                text = message,
                packageName = CollectorPreferences.foregroundPackage(context),
                className = CollectorPreferences.foregroundClass(context),
                deviceContext = DeviceContextCollector.snapshot(context),
                rawPayload = rawPayload,
            ),
        )
    }
}
