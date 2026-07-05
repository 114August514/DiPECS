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

        val result = prewarmOwnedResources(appContext, normalizedTarget)

        EventRepository.recordInternal(
            appContext,
            "own_resources_prewarmed",
            "Prewarmed DiPECS-owned resources",
            JSONObject()
                .put("target", normalizedTarget)
                .put("reason", reason)
                .put("traceRows", result.traceRows)
                .put("prefetchCacheReady", result.prefetchCacheReady)
                .put("volatileCacheBytes", result.volatileCacheBytes)
                .put("warmedComponents", result.warmedComponents),
        )
        return true
    }

    private fun prewarmOwnedResources(context: Context, target: String): PrewarmResult {
        val traceStats = EventStore(context).stats()
        val prefetchDir = File(context.cacheDir, "prefetch").apply {
            if (!exists()) {
                mkdirs()
            }
        }
        val volatileCacheBytes = if (target.startsWith(TARGET_VOLATILE_CACHE)) {
            VolatileMemoryCache.seed(VolatileMemoryCache.parseTargetMb(target)).heldBytes
        } else {
            0L
        }
        val warmedComponents = listOf(
            AccessibleContentPrefetcher::class.java.name,
            ActionMaintenanceScheduler::class.java.name,
            CacheTrimmer::class.java.name,
            VolatileMemoryCache::class.java.name,
            UserVisibleActionNotifier::class.java.name,
        )
        CollectorPreferences.actionSocketToken(context)
        CollectorPreferences.prefetchTarget(context)
        CollectorPreferences.actionSocketPort(context)

        return PrewarmResult(
            traceRows = traceStats.totalRows,
            prefetchCacheReady = prefetchDir.exists(),
            volatileCacheBytes = volatileCacheBytes,
            warmedComponents = warmedComponents,
        )
    }

    private fun isAllowedTarget(target: String): Boolean =
        target == TARGET_RESOURCES || target.startsWith("own:")

    private data class PrewarmResult(
        val traceRows: Int,
        val prefetchCacheReady: Boolean,
        val volatileCacheBytes: Long,
        val warmedComponents: List<String>,
    )

    private const val TARGET_RESOURCES = "own:resources"
    private const val TARGET_VOLATILE_CACHE = "own:volatile-cache"
}
