package com.dipecs.collector.actions

import android.app.ActivityManager
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject
import java.io.BufferedReader
import java.io.File
import java.io.InputStreamReader
import java.util.concurrent.TimeUnit

/**
 * System-level action executors for a platform-signed DiPECS system daemon.
 *
 * When `dipecsd` runs at `/system/bin/dipecsd` and the Kotlin bridge is signed
 * with the platform certificate, these implementations use system-only APIs
 * (ActivityManager, /proc writes, pm shell) that a normal app cannot access.
 *
 * Each function returns `ActionResult` so the bridge can produce an honest
 * `BridgeExecuteResponse` back to the Rust side.
 */
object SystemActionExecutors {

    data class ActionResult(
        val success: Boolean,
        val summary: String,
        val latencyUs: Long,
        val error: String?,
    )

    private const val OOM_SCORE_ADJ_PATH = "/proc/%d/oom_score_adj"
    private const val CPUSET_FOREGROUND_TASKS = "/dev/cpuset/foreground/tasks"
    private const val DROP_CACHES_PATH = "/proc/sys/vm/drop_caches"
    private const val PREWARM_POLL_INTERVAL_MS = 200L
    private const val PREWARM_MAX_RETRIES = 10

    // ──────────────────────────────────────────────────
    //  PreWarmProcess  —  Zygote fork via dummy Activity
    // ──────────────────────────────────────────────────

    fun prewarmProcess(
        context: Context,
        target: String,
        reason: String,
    ): ActionResult {
        val startedAt = System.nanoTime()
        val appContext = context.applicationContext
        val pkg = parsePackageTarget(target)

        // If target is "own:*", warm DiPECS's own helper components via our
        // internal SystemPrewarmActivity.
        return if (pkg == appContext.packageName) {
            prewarmOwn(context, target, reason, startedAt)
        } else {
            prewarmExternal(appContext, pkg, target, reason, startedAt)
        }
    }

    private fun prewarmOwn(
        context: Context,
        target: String,
        reason: String,
        startedAt: Long,
    ): ActionResult {
        if (target.startsWith("own:volatile-cache")) {
            val ok = OwnResourceWarmer.warm(context, target, reason)
            return ActionResult(
                success = ok,
                summary = if (ok) "prewarm_own:${target.removePrefix("own:")}" else "prewarm_own_failed",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = if (ok) null else "Volatile cache prewarm rejected",
            )
        }
        val intent = Intent(context, SystemPrewarmActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            putExtra(SystemPrewarmActivity.EXTRA_TARGET, target)
            putExtra(SystemPrewarmActivity.EXTRA_REASON, reason)
        }
        return runCatching {
            context.startActivity(intent)
            ActionResult(
                success = true,
                summary = "prewarm_own:${target.removePrefix("own:")}",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = null,
            )
        }.getOrElse { error ->
            ActionResult(
                success = false,
                summary = "prewarm_own_failed",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = error.message,
            )
        }
    }

    private fun prewarmExternal(
        context: Context,
        pkg: String,
        target: String,
        reason: String,
        startedAt: Long,
    ): ActionResult {
        return try {
            // Launch the pkg's LAUNCHER activity in a new task, then immediately
            // kill the task. The fork+Application.onCreate has already happened.
            val launchIntent = context.packageManager
                .getLaunchIntentForPackage(pkg)
            if (launchIntent == null) {
                return ActionResult(
                    success = false,
                    summary = "prewarm_no_launcher:$pkg",
                    latencyUs = (System.nanoTime() - startedAt) / 1000,
                    error = "Package $pkg has no launcher activity",
                )
            }
            launchIntent.addFlags(
                Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_CLEAR_TOP,
            )
            context.startActivity(launchIntent)

            val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
            // Poll for the launched task with retry. The Zygote fork+onCreate
            // may take >200 ms on a cold start.
            var taskFound = false
            for (i in 0 until PREWARM_MAX_RETRIES) {
                Thread.sleep(PREWARM_POLL_INTERVAL_MS)
                taskFound = am.appTasks.any { it.taskInfo?.baseIntent?.`package` == pkg }
                if (taskFound) break
            }
            if (taskFound) {
                am.appTasks.firstOrNull { it.taskInfo?.baseIntent?.`package` == pkg }
                    ?.finishAndRemoveTask()
            }

            ActionResult(
                success = true,
                summary = "prewarm:$pkg",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = null,
            )
        } catch (error: SecurityException) {
            // Normal app without INTERACT_ACROSS_USERS — can't launch background.
            ActionResult(
                success = false,
                summary = "prewarm_security_denied:$pkg",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = "Start background activity denied: ${error.message}",
            )
        } catch (error: Exception) {
            ActionResult(
                success = false,
                summary = "prewarm_failed:$pkg",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = error.message,
            )
        }
    }

    private fun parsePackageTarget(target: String): String {
        if (target.startsWith("own:")) {
            return "com.dipecs.collector"
        }
        return target.removePrefix("pkg:")
            .removePrefix("notif:")
            .substringBefore(":")
            .substringBefore("/")
            .ifBlank { "com.dipecs.collector" }
    }

    // ──────────────────────────────────────────────────
    //  KeepAlive  —  OOM score lowering + cgroup pin
    // ──────────────────────────────────────────────────

    fun keepAlive(
        context: Context,
        target: String,
        reason: String,
    ): ActionResult {
        val startedAt = System.nanoTime()
        val appContext = context.applicationContext

        // System-level: lower our own OOM score so LMKD won't kill us.
        val pid = android.os.Process.myPid()
        val oomOk = try {
            writeOomScoreAdj(pid, -800)
            true
        } catch (_: Exception) {
            false
        }
        val cgOk = pinToForegroundCgroup(pid)

        // Always also schedule the JobScheduler keepalive as fallback.
        ActionMaintenanceScheduler.schedule(appContext, target, reason)

        val success = oomOk || cgOk

        EventRepository.recordInternal(
            appContext,
            if (success) "keep_alive_system" else "keep_alive_fallback",
            if (success) "System KeepAlive: oom=$oomOk cgroup=$cgOk" else "KeepAlive fell back to JobScheduler",
            JSONObject()
                .put("target", target)
                .put("reason", reason)
                .put("oomScoreAdjusted", oomOk)
                .put("cgroupPinned", cgOk),
        )

        return ActionResult(
            success = success,
            summary = buildString {
                append("keepalive")
                if (oomOk) append(":oom")
                if (cgOk) append(":cgroup")
                if (!success) append(":fallback")
            },
            latencyUs = (System.nanoTime() - startedAt) / 1000,
            error = if (success) null else "oom=denied, cgroup=denied — JobScheduler fallback only",
        )
    }

    private fun writeOomScoreAdj(pid: Int, score: Int) {
        val path = OOM_SCORE_ADJ_PATH.format(pid)
        File(path).writeText("$score\n")
    }

    private fun pinToForegroundCgroup(pid: Int): Boolean {
        val cgroupFile = File(CPUSET_FOREGROUND_TASKS)
        if (!cgroupFile.exists()) {
            return false
        }
        return try {
            cgroupFile.appendText("$pid\n")
            true
        } catch (_: Exception) {
            false
        }
    }

    // ──────────────────────────────────────────────────
    //  ReleaseMemory  —  app-cache clearing + drop_caches
    // ──────────────────────────────────────────────────

    fun releaseMemory(
        context: Context,
        target: String?,
        reason: String,
    ): ActionResult {
        val startedAt = System.nanoTime()
        val appContext = context.applicationContext
        val normalizedTarget = target?.trim().takeUnless { it.isNullOrBlank() } ?: "cache:prefetch"
        val parts = mutableListOf<String>()
        var error: String? = null

        when {
            normalizedTarget == "cache:prefetch" -> {
                val deleted = AccessibleContentPrefetcher.clearCache(appContext)
                parts += "prefetch_cache:$deleted"
            }
            normalizedTarget == "cache:volatile" -> {
                val released = VolatileMemoryCache.clear()
                requestHeapTrim()
                parts += "volatile_cache:${released.releasedBytes}"
            }
            normalizedTarget == "cache:all" -> {
                // 1. Clear our own prefetch cache.
                val deleted = AccessibleContentPrefetcher.clearCache(appContext)
                parts += "prefetch_cache:$deleted"
                val released = VolatileMemoryCache.clear()
                requestHeapTrim()
                parts += "volatile_cache:${released.releasedBytes}"
                // 2. Try clearing other app caches (system-only).
                try {
                    val cleared = clearAppCachesSystem(appContext)
                    parts += "app_caches:$cleared"
                } catch (e: Exception) {
                    error = "cache:all system clear failed: ${e.message}"
                    parts += "app_caches:denied"
                }
            }
            normalizedTarget.startsWith("pkg:") -> {
                val pkg = normalizedTarget.removePrefix("pkg:")
                try {
                    val result = clearPackageCache(appContext, pkg)
                    parts += "pkg:$pkg:$result"
                } catch (e: Exception) {
                    error = e.message
                    parts += "pkg:$pkg:denied"
                }
            }
            normalizedTarget == "page" -> {
                EventRepository.recordInternal(
                    appContext,
                    "release_memory_drop_caches",
                    "System-wide drop_caches requested — all app page caches will be evicted",
                    JSONObject().put("target", normalizedTarget).put("reason", reason),
                )
                try {
                    dropPageCache()
                    parts += "drop_caches"
                } catch (e: Exception) {
                    error = e.message
                    parts += "drop_caches:denied"
                }
            }
            else -> {
                // Fall back to the normal-app CacheTrimmer.
                val deleted = CacheTrimmer.release(appContext, target, reason)
                parts += "app_cache:$deleted"
            }
        }

        EventRepository.recordInternal(
            appContext,
            "release_memory_system",
            "System ReleaseMemory executed",
            JSONObject()
                .put("target", normalizedTarget)
                .put("reason", reason)
                .put("parts", parts.joinToString(",")),
        )

        val summary = parts.joinToString(";")
        return ActionResult(
            success = error == null,
            summary = summary,
            latencyUs = (System.nanoTime() - startedAt) / 1000,
            error = error,
        )
    }

    private fun clearAppCachesSystem(context: Context): Int {
        val pm = context.packageManager
        val packages = pm.getInstalledApplications(PackageManager.GET_META_DATA)
        var count = 0
        var failureCount = 0
        val failureMessages = mutableListOf<String>()
        for (app in packages) {
            val cleared = try {
                // deleteApplicationCacheFiles is hidden API — use reflection.
                val observerClass = Class.forName("android.content.pm.IPackageDataObserver")
                val deleteMethod = PackageManager::class.java.getDeclaredMethod(
                    "deleteApplicationCacheFiles",
                    java.lang.String::class.java,
                    observerClass,
                )
                deleteMethod.invoke(pm, app.packageName, null)
                true
            } catch (e: Exception) {
                failureCount++
                if (failureMessages.size < 5) {
                    failureMessages += "${app.packageName}: ${e.javaClass.simpleName}"
                }
                false
            }
            if (cleared) count++
        }
        if (failureCount > 0) {
            EventRepository.recordInternal(
                context,
                "release_memory_reflection_failures",
                "deleteApplicationCacheFiles reflection failed for $failureCount packages",
                JSONObject()
                    .put("failureCount", failureCount)
                    .put("totalPackages", packages.size)
                    .put("sampleFailures", failureMessages.joinToString("; ")),
            )
        }
        return count
    }

    private fun requestHeapTrim() {
        Runtime.getRuntime().gc()
        System.runFinalization()
        Runtime.getRuntime().gc()
    }

    private fun clearPackageCache(context: Context, pkg: String): String {
        return try {
            val process = Runtime.getRuntime().exec(
                arrayOf("pm", "clear", "--cache-only", pkg),
            )
            val reader = BufferedReader(InputStreamReader(process.inputStream))
            val output = reader.readText().trim()
            reader.close()
            if (!process.waitFor(5, TimeUnit.SECONDS)) {
                process.destroyForcibly()
                return "pm_clear_timeout:$pkg"
            }
            output.ifBlank { "cleared" }
        } catch (e: Exception) {
            error("pm clear $pkg: ${e.message}")
        }
    }

    private fun dropPageCache() {
        val file = File(DROP_CACHES_PATH)
        if (!file.canWrite()) {
            error("Cannot write $DROP_CACHES_PATH — not running as root")
        }
        // Drops the system-wide page cache — all apps are affected,
        // not just the target. Avoid in production.
        file.writeText("1\n")
    }

    // ──────────────────────────────────────────────────
    //  PrefetchFile  —  delegates to existing prefetcher
    // ──────────────────────────────────────────────────

    fun prefetchFile(
        context: Context,
        target: String,
        reason: String,
        onResult: (ActionResult) -> Unit,
    ) {
        val startedAt = System.nanoTime()
        val appContext = context.applicationContext

        // AccessibleContentPrefetcher.enqueue is async; we wrap it with a
        // completed-ack and the real result comes via EventRepository.
        AccessibleContentPrefetcher.enqueue(appContext, target, reason)

        // Return a synchronous "acknowledged" outcome. The real download
        // result is too slow to fit in a TCP request/response cycle
        // (5 s timeout). Device-side auditing captures the full outcome.
        onResult(
            ActionResult(
                success = true,
                summary = "prefetch_enqueued",
                latencyUs = (System.nanoTime() - startedAt) / 1000,
                error = null,
            ),
        )
    }

    // ──────────────────────────────────────────────────
    //  NoOp
    // ──────────────────────────────────────────────────

    fun noOp(context: Context, reason: String): ActionResult {
        val startedAt = System.nanoTime()
        EventRepository.recordInternal(
            context.applicationContext,
            "action_noop",
            "System NoOp action acknowledged",
            JSONObject().put("reason", reason),
        )
        return ActionResult(
            success = true,
            summary = "noop",
            latencyUs = (System.nanoTime() - startedAt) / 1000,
            error = null,
        )
    }
}
