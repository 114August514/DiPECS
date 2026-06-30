package com.dipecs.collector.services

import android.app.job.JobParameters
import android.app.job.JobService
import com.dipecs.collector.collectors.DeviceContextCollector
import com.dipecs.collector.model.AndroidRawEventMapper
import com.dipecs.collector.model.CollectorEvent
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

class ActionMaintenanceJobService : JobService() {
    override fun onStartJob(params: JobParameters?): Boolean {
        val target = params?.extras?.getString(EXTRA_TARGET) ?: "work:collector_heartbeat"
        val reason = params?.extras?.getString(EXTRA_REASON) ?: "job_scheduler"
        val now = System.currentTimeMillis()
        val deviceContext = DeviceContextCollector.snapshot(this)

        CollectorPreferences.setLastHeartbeatMs(this, now)
        EventRepository.record(
            this,
            CollectorEvent(
                timestampMs = now,
                source = "internal",
                eventType = "keep_alive_job_executed",
                text = "DiPECS-owned maintenance job executed",
                deviceContext = deviceContext,
                rawEvent = AndroidRawEventMapper.systemState(now, deviceContext),
                rawPayload = JSONObject()
                    .put("target", target)
                    .put("reason", reason),
            ),
        )
        jobFinished(params, false)
        return false
    }

    override fun onStopJob(params: JobParameters?): Boolean = false

    companion object {
        const val EXTRA_TARGET = "target"
        const val EXTRA_REASON = "reason"
    }
}
