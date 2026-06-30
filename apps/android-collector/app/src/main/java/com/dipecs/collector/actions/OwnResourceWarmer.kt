package com.dipecs.collector.actions

import android.content.Context
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import com.dipecs.collector.storage.EventStore
import java.io.File
import org.json.JSONObject

object OwnResourceWarmer {
    fun warm(context: Context, target: String?, reason: String): Boolean {
        val appContext = context.applicationContext
        val normalizedTarget = target?.trim().takeUnless { it.isNullOrBlank() } ?: TARGET_RESOURCES
        if (!isAllowedTarget(normalizedTarget)) {
            EventRepository.recordInternal(
                appContext,
                "prewarm_rejected",
                "PreWarmProcess target is not DiPECS-owned",
                JSONObject()
                    .put("target", normalizedTarget)
                    .put("reason", reason),
            )
            return false
        }

        val traceStats = EventStore(appContext).stats()
        val prefetchDir = File(appContext.cacheDir, "prefetch")
        if (!prefetchDir.exists()) {
            prefetchDir.mkdirs()
        }
        CollectorPreferences.actionSocketToken(appContext)

        EventRepository.recordInternal(
            appContext,
            "own_resources_prewarmed",
            "Prewarmed DiPECS-owned resources",
            JSONObject()
                .put("target", normalizedTarget)
                .put("reason", reason)
                .put("traceRows", traceStats.totalRows)
                .put("prefetchCacheReady", prefetchDir.exists()),
        )
        return true
    }

    private fun isAllowedTarget(target: String): Boolean =
        target == TARGET_RESOURCES || target.startsWith("own:")

    private const val TARGET_RESOURCES = "own:resources"
}
