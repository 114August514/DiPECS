package com.dipecs.collector.actions

import android.app.job.JobInfo
import android.app.job.JobScheduler
import android.content.ComponentName
import android.content.Context
import android.os.PersistableBundle
import com.dipecs.collector.services.ActionMaintenanceJobService
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

object ActionMaintenanceScheduler {
    fun schedule(context: Context, target: String?, reason: String): Boolean {
        val appContext = context.applicationContext
        val normalizedTarget = target?.trim().takeUnless { it.isNullOrBlank() } ?: TARGET_HEARTBEAT
        if (!isAllowedTarget(normalizedTarget)) {
            EventRepository.recordInternal(
                appContext,
                "keep_alive_rejected",
                "KeepAlive target is outside DiPECS-owned work",
                JSONObject()
                    .put("target", normalizedTarget)
                    .put("reason", reason),
            )
            return false
        }

        val scheduler = appContext.getSystemService(JobScheduler::class.java)
        if (scheduler == null) {
            EventRepository.recordInternal(
                appContext,
                "keep_alive_failed",
                "JobScheduler unavailable",
                JSONObject()
                    .put("target", normalizedTarget)
                    .put("reason", reason),
            )
            return false
        }

        val info = JobInfo.Builder(
            JOB_ID,
            ComponentName(appContext, ActionMaintenanceJobService::class.java),
        )
            .setMinimumLatency(MIN_LATENCY_MS)
            .setOverrideDeadline(OVERRIDE_DEADLINE_MS)
            .setRequiredNetworkType(JobInfo.NETWORK_TYPE_NONE)
            .setExtras(
                PersistableBundle().apply {
                    putString(ActionMaintenanceJobService.EXTRA_TARGET, normalizedTarget)
                    putString(ActionMaintenanceJobService.EXTRA_REASON, reason)
                },
            )
            .build()

        val result = scheduler.schedule(info)
        val scheduled = result == JobScheduler.RESULT_SUCCESS
        EventRepository.recordInternal(
            appContext,
            if (scheduled) "keep_alive_scheduled" else "keep_alive_failed",
            if (scheduled) "Scheduled DiPECS-owned maintenance job" else "JobScheduler rejected maintenance job",
            JSONObject()
                .put("target", normalizedTarget)
                .put("reason", reason)
                .put("jobId", JOB_ID),
        )
        return scheduled
    }

    private fun isAllowedTarget(target: String): Boolean =
        target == TARGET_HEARTBEAT || target.startsWith("work:")

    private const val TARGET_HEARTBEAT = "work:collector_heartbeat"
    private const val JOB_ID = 4632101
    private const val MIN_LATENCY_MS = 1_000L
    private const val OVERRIDE_DEADLINE_MS = 5_000L
}
