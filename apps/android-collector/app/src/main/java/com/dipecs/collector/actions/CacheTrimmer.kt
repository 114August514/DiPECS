package com.dipecs.collector.actions

import android.content.Context
import com.dipecs.collector.storage.EventRepository
import java.io.File
import org.json.JSONObject

object CacheTrimmer {
    fun release(context: Context, target: String?, reason: String): Int {
        val appContext = context.applicationContext
        val normalizedTarget = target?.trim().takeUnless { it.isNullOrBlank() } ?: TARGET_PREFETCH
        val deleted = when (normalizedTarget) {
            TARGET_PREFETCH -> AccessibleContentPrefetcher.clearCache(appContext)
            TARGET_ALL -> clearDirectoryChildren(appContext.cacheDir)
            else -> {
                EventRepository.recordInternal(
                    appContext,
                    "release_memory_rejected",
                    "ReleaseMemory target is outside app-owned cache",
                    JSONObject()
                        .put("target", normalizedTarget)
                        .put("reason", reason),
                )
                return 0
            }
        }

        EventRepository.recordInternal(
            appContext,
            "release_memory_completed",
            "Released app-owned cache",
            JSONObject()
                .put("target", normalizedTarget)
                .put("reason", reason)
                .put("deletedFiles", deleted),
        )
        return deleted
    }

    private fun clearDirectoryChildren(dir: File): Int {
        if (!dir.exists()) {
            return 0
        }
        return dir.listFiles()
            ?.count { child -> deleteRecursively(child) }
            ?: 0
    }

    private fun deleteRecursively(file: File): Boolean {
        if (file.isDirectory) {
            file.listFiles()?.forEach { deleteRecursively(it) }
        }
        return file.delete()
    }

    private const val TARGET_PREFETCH = "cache:prefetch"
    private const val TARGET_ALL = "cache:all"
}
