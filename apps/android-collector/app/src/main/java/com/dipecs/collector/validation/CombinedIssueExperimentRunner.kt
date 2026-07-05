package com.dipecs.collector.validation

import android.app.ActivityManager
import android.content.Context
import android.os.Debug
import android.os.Process
import com.dipecs.collector.actions.AccessibleContentPrefetcher
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.ceil

class CombinedIssueExperimentRunner(
    context: Context,
    private val listener: Listener,
) {
    interface Listener {
        fun onCombinedSnapshot(snapshot: CombinedExperimentSnapshot)
        fun onCombinedFinished(snapshot: CombinedExperimentSnapshot)
    }

    private val appContext = context.applicationContext
    private val cancelRequested = AtomicBoolean(false)
    private val samples = mutableListOf<CombinedExperimentSample>()
    private var worker: Thread? = null
    private var startedAtMs = 0L
    private var targetDurationMs = 0L
    private var intervalMs = 0L
    private var prefetchTarget = ""
    private var lastReport = ""
    private var lastJsonPath = ""
    private var lastMarkdownPath = ""

    fun start(durationMinutes: Int, intervalSeconds: Int, target: String) {
        if (worker?.isAlive == true) return
        require(durationMinutes > 0) { "durationMinutes must be positive" }
        require(intervalSeconds > 0) { "intervalSeconds must be positive" }

        cancelRequested.set(false)
        samples.clear()
        startedAtMs = System.currentTimeMillis()
        targetDurationMs = durationMinutes * 60_000L
        intervalMs = intervalSeconds * 1_000L
        prefetchTarget = target.trim()
        lastReport = ""
        lastJsonPath = ""
        lastMarkdownPath = ""

        worker = Thread {
            enforceLocalOnlyMode()
            EventRepository.recordInternal(
                appContext,
                "combined_issue_experiment_started",
                "Started combined #97/#98/#99 experiment",
                JSONObject()
                    .put("durationMinutes", durationMinutes)
                    .put("intervalSeconds", intervalSeconds)
                    .put("prefetchTarget", prefetchTarget.ifBlank { JSONObject.NULL }),
            )
            publish("running")
            var index = 0
            while (!cancelRequested.get() && System.currentTimeMillis() - startedAtMs < targetDurationMs) {
                val sample = runOneSample(index)
                synchronized(samples) { samples += sample }
                publish("sample ${index + 1} collected")
                index += 1
                sleepInterruptibly(intervalMs)
            }
            export()
            listener.onCombinedFinished(snapshot("finished"))
        }.apply {
            name = "dipecs-combined-issue-experiment"
            start()
        }
    }

    fun stop() {
        cancelRequested.set(true)
        publish("stop requested")
    }

    fun snapshot(message: String = ""): CombinedExperimentSnapshot {
        val copy = synchronized(samples) { samples.toList() }
        val summary = CombinedExperimentSummary.from(copy)
        return CombinedExperimentSnapshot(
            running = worker?.isAlive == true && !cancelRequested.get(),
            startedAtMs = startedAtMs,
            elapsedMs = if (startedAtMs > 0L) System.currentTimeMillis() - startedAtMs else 0L,
            targetDurationMs = targetDurationMs,
            intervalMs = intervalMs,
            prefetchTarget = prefetchTarget,
            message = message,
            samples = copy,
            summary = summary,
            markdownReport = lastReport.ifBlank { buildMarkdownReport(copy, summary) },
            jsonPath = lastJsonPath,
            markdownPath = lastMarkdownPath,
        )
    }

    private fun runOneSample(index: Int): CombinedExperimentSample {
        val before = DeviceSample.capture(appContext)
        val started = System.currentTimeMillis()

        val prefetchResult = runPrefetch(index)
        val keepAliveResult = runKeepAlive()
        seedPrefetchCache(index)
        val releaseResult = runReleaseMemory()

        val after = DeviceSample.capture(appContext)
        return CombinedExperimentSample(
            index = index,
            timestampMs = started,
            elapsedMs = started - startedAtMs,
            before = before,
            after = after,
            prefetch = prefetchResult,
            keepAlive = keepAliveResult,
            releaseMemory = releaseResult,
        )
    }

    private fun runPrefetch(index: Int): ActionMetric {
        if (prefetchTarget.isBlank()) {
            return ActionMetric.skipped("target blank")
        }
        val before = cacheStats()
        val started = System.currentTimeMillis()
        val result = ActionExecutorBridge.dispatch(
            appContext,
            ActionExecutorBridge.ACTION_TYPE_PREFETCH_FILE,
            prefetchTarget,
            reason = "combined_issue_experiment_97",
        )
        waitUntil(20_000L) {
            cancelRequested.get() || cacheStats().bytes > before.bytes ||
                recentHas("prefetch_succeeded", "combined_issue_experiment_97")
        }
        val after = cacheStats()
        val succeeded = result.success && after.bytes >= before.bytes
        return ActionMetric(
            attempted = true,
            success = succeeded,
            latencyMs = System.currentTimeMillis() - started,
            dispatchLatencyUs = result.latencyUs,
            summary = result.summary,
            beforeValue = before.bytes,
            afterValue = after.bytes,
            deltaValue = after.bytes - before.bytes,
            note = "cache_bytes",
        )
    }

    private fun runKeepAlive(): ActionMetric {
        val beforeHeartbeat = CollectorPreferences.lastHeartbeatMs(appContext)
        val started = System.currentTimeMillis()
        val result = ActionExecutorBridge.dispatch(
            appContext,
            ActionExecutorBridge.ACTION_TYPE_KEEP_ALIVE,
            "work:collector_heartbeat",
            reason = "combined_issue_experiment_98",
        )
        waitUntil(7_000L) {
            cancelRequested.get() ||
                CollectorPreferences.lastHeartbeatMs(appContext) > beforeHeartbeat ||
                recentHas("keep_alive_scheduled", "combined_issue_experiment_98")
        }
        val afterHeartbeat = CollectorPreferences.lastHeartbeatMs(appContext)
        val success = result.success || afterHeartbeat > beforeHeartbeat ||
            recentHas("keep_alive_scheduled", "combined_issue_experiment_98")
        return ActionMetric(
            attempted = true,
            success = success,
            latencyMs = System.currentTimeMillis() - started,
            dispatchLatencyUs = result.latencyUs,
            summary = result.summary,
            beforeValue = beforeHeartbeat,
            afterValue = afterHeartbeat,
            deltaValue = afterHeartbeat - beforeHeartbeat,
            note = "heartbeat_ms",
        )
    }

    private fun runReleaseMemory(): ActionMetric {
        val before = cacheStats()
        val beforeMem = DeviceSample.capture(appContext)
        val started = System.currentTimeMillis()
        val result = ActionExecutorBridge.dispatch(
            appContext,
            ActionExecutorBridge.ACTION_TYPE_RELEASE_MEMORY,
            "cache:prefetch",
            reason = "combined_issue_experiment_99",
        )
        val after = cacheStats()
        val afterMem = DeviceSample.capture(appContext)
        val released = after.bytes < before.bytes
        return ActionMetric(
            attempted = true,
            success = result.success && released,
            latencyMs = System.currentTimeMillis() - started,
            dispatchLatencyUs = result.latencyUs,
            summary = result.summary,
            beforeValue = before.bytes,
            afterValue = after.bytes,
            deltaValue = afterMem.availableMemKb - beforeMem.availableMemKb,
            note = "cache_bytes_before_after;available_mem_delta_kb",
        )
    }

    private fun seedPrefetchCache(index: Int) {
        val dir = File(appContext.cacheDir, "prefetch")
        dir.mkdirs()
        File(dir, "combined-release-seed-$index.bin").writeBytes(ByteArray(64 * 1024) { index.toByte() })
    }

    private fun export() {
        val copy = synchronized(samples) { samples.toList() }
        val summary = CombinedExperimentSummary.from(copy)
        val outDir = File(appContext.getExternalFilesDir(null) ?: appContext.filesDir, "validation")
        outDir.mkdirs()
        val ts = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val json = File(outDir, "combined-issues-$ts.jsonl")
        json.bufferedWriter().use { writer ->
            copy.forEach { sample ->
                writer.write(sample.toJson().toString())
                writer.newLine()
            }
        }
        val md = File(outDir, "combined-issues-$ts.md")
        lastReport = buildMarkdownReport(copy, summary)
        md.writeText(lastReport)
        lastJsonPath = json.absolutePath
        lastMarkdownPath = md.absolutePath
        EventRepository.recordInternal(
            appContext,
            "combined_issue_experiment_exported",
            "Exported combined experiment result",
            JSONObject()
                .put("jsonPath", lastJsonPath)
                .put("markdownPath", lastMarkdownPath)
                .put("samples", copy.size),
        )
    }

    private fun buildMarkdownReport(
        samples: List<CombinedExperimentSample>,
        summary: CombinedExperimentSummary,
    ): String = buildString {
        appendLine("# DiPECS #97/#98/#99 Combined Device Experiment")
        appendLine()
        appendLine("- Samples: ${summary.samples}")
        appendLine("- Elapsed: ${summary.elapsedMinutesText}")
        appendLine("- Prefetch target: ${prefetchTarget.ifBlank { "not set" }}")
        appendLine("- JSONL: ${lastJsonPath.ifBlank { "not exported yet" }}")
        appendLine("- Markdown: ${lastMarkdownPath.ifBlank { "not exported yet" }}")
        appendLine()
        appendLine("## Summary")
        appendLine()
        appendLine("| Metric | Value |")
        appendLine("| --- | ---: |")
        appendLine("| Prefetch success rate | ${summary.prefetchSuccessRateText} |")
        appendLine("| Prefetch mean latency | ${summary.prefetchMeanLatencyMs} ms |")
        appendLine("| KeepAlive success rate | ${summary.keepAliveSuccessRateText} |")
        appendLine("| KeepAlive mean latency | ${summary.keepAliveMeanLatencyMs} ms |")
        appendLine("| ReleaseMemory success rate | ${summary.releaseSuccessRateText} |")
        appendLine("| ReleaseMemory mean available-mem delta | ${summary.releaseMeanAvailableDeltaKb} KB |")
        appendLine("| Mean PSS delta | ${summary.meanPssDeltaKb} KB |")
        appendLine("| Mean Java heap delta | ${summary.meanHeapDeltaKb} KB |")
        appendLine()
        appendLine("## Detailed Samples")
        appendLine()
        appendLine("| # | elapsed min | Prefetch | KeepAlive | ReleaseMemory | PSS delta KB | Heap delta KB | Available delta KB |")
        appendLine("| ---: | ---: | --- | --- | --- | ---: | ---: | ---: |")
        samples.forEach { sample ->
            appendLine(
                "| ${sample.index} | ${String.format(Locale.US, "%.1f", sample.elapsedMs / 60_000.0)} | " +
                    "${sample.prefetch.statusText} | ${sample.keepAlive.statusText} | " +
                    "${sample.releaseMemory.statusText} | ${sample.after.pssKb - sample.before.pssKb} | " +
                    "${sample.after.javaHeapUsedKb - sample.before.javaHeapUsedKb} | " +
                    "${sample.after.availableMemKb - sample.before.availableMemKb} |",
            )
        }
        appendLine()
        appendLine("## Notes")
        appendLine()
        appendLine("- This in-app run proves device-side action behavior and local impact over time.")
        appendLine("- It does not replace adb pressure scripts for strict #98/#99 memory-pressure acceptance.")
        appendLine("- Use the exported JSONL/Markdown as phone-side evidence and copy it into the project.")
    }

    private fun publish(message: String) {
        listener.onCombinedSnapshot(snapshot(message))
    }

    private fun enforceLocalOnlyMode() {
        CollectorPreferences.setUploadMode(appContext, CollectorPreferences.MODE_MOCK)
        CollectorPreferences.setUploadEnabled(appContext, false)
        CollectorPreferences.setApiKey(appContext, "")
    }

    private fun waitUntil(timeoutMs: Long, predicate: () -> Boolean) {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline && !cancelRequested.get()) {
            if (predicate()) return
            Thread.sleep(250L)
        }
    }

    private fun sleepInterruptibly(ms: Long) {
        val deadline = System.currentTimeMillis() + ms
        while (!cancelRequested.get() && System.currentTimeMillis() < deadline) {
            Thread.sleep(500L.coerceAtMost(deadline - System.currentTimeMillis()))
        }
    }

    private fun recentHas(eventType: String, reason: String): Boolean =
        com.dipecs.collector.storage.EventStore(appContext).readRecent(96).any {
            it.optString("eventType") == eventType &&
                it.optJSONObject("rawPayload")?.optString("reason") == reason
        }

    private fun cacheStats(): CacheStats {
        val dir = File(appContext.cacheDir, "prefetch")
        if (!dir.exists()) return CacheStats(0, 0)
        var files = 0
        var bytes = 0L
        dir.walkTopDown().forEach { file ->
            if (file.isFile) {
                files += 1
                bytes += file.length()
            }
        }
        return CacheStats(files, bytes)
    }

    private data class CacheStats(val files: Int, val bytes: Long)
}

data class CombinedExperimentSnapshot(
    val running: Boolean,
    val startedAtMs: Long,
    val elapsedMs: Long,
    val targetDurationMs: Long,
    val intervalMs: Long,
    val prefetchTarget: String,
    val message: String,
    val samples: List<CombinedExperimentSample>,
    val summary: CombinedExperimentSummary,
    val markdownReport: String,
    val jsonPath: String,
    val markdownPath: String,
)

data class CombinedExperimentSample(
    val index: Int,
    val timestampMs: Long,
    val elapsedMs: Long,
    val before: DeviceSample,
    val after: DeviceSample,
    val prefetch: ActionMetric,
    val keepAlive: ActionMetric,
    val releaseMemory: ActionMetric,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("schema_version", "dipecs.combined_issue_experiment.sample.v1")
            .put("index", index)
            .put("timestamp_ms", timestampMs)
            .put("elapsed_ms", elapsedMs)
            .put("before", before.toJson())
            .put("after", after.toJson())
            .put("prefetch", prefetch.toJson())
            .put("keep_alive", keepAlive.toJson())
            .put("release_memory", releaseMemory.toJson())
}

data class DeviceSample(
    val timestampMs: Long,
    val pssKb: Int,
    val javaHeapUsedKb: Long,
    val javaHeapMaxKb: Long,
    val availableMemKb: Long,
    val lowMemory: Boolean,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("pss_kb", pssKb)
            .put("java_heap_used_kb", javaHeapUsedKb)
            .put("java_heap_max_kb", javaHeapMaxKb)
            .put("available_mem_kb", availableMemKb)
            .put("low_memory", lowMemory)

    companion object {
        fun capture(context: Context): DeviceSample {
            val memoryInfo = Debug.MemoryInfo()
            Debug.getMemoryInfo(memoryInfo)
            val runtime = Runtime.getRuntime()
            val activityManager = context.getSystemService(ActivityManager::class.java)
            val pss = activityManager
                ?.getProcessMemoryInfo(intArrayOf(Process.myPid()))
                ?.firstOrNull()
                ?.totalPss
                ?: memoryInfo.totalPss
            val systemMem = ActivityManager.MemoryInfo()
            activityManager?.getMemoryInfo(systemMem)
            return DeviceSample(
                timestampMs = System.currentTimeMillis(),
                pssKb = pss,
                javaHeapUsedKb = (runtime.totalMemory() - runtime.freeMemory()) / 1024,
                javaHeapMaxKb = runtime.maxMemory() / 1024,
                availableMemKb = systemMem.availMem / 1024,
                lowMemory = systemMem.lowMemory,
            )
        }
    }
}

data class ActionMetric(
    val attempted: Boolean,
    val success: Boolean,
    val latencyMs: Long,
    val dispatchLatencyUs: Long,
    val summary: String,
    val beforeValue: Long,
    val afterValue: Long,
    val deltaValue: Long,
    val note: String,
) {
    val statusText: String
        get() = when {
            !attempted -> "skipped"
            success -> "ok"
            else -> "failed"
        }

    fun toJson(): JSONObject =
        JSONObject()
            .put("attempted", attempted)
            .put("success", success)
            .put("latency_ms", latencyMs)
            .put("dispatch_latency_us", dispatchLatencyUs)
            .put("summary", summary)
            .put("before_value", beforeValue)
            .put("after_value", afterValue)
            .put("delta_value", deltaValue)
            .put("note", note)

    companion object {
        fun skipped(reason: String): ActionMetric =
            ActionMetric(false, false, 0, 0, "skipped", 0, 0, 0, reason)
    }
}

data class CombinedExperimentSummary(
    val samples: Int,
    val elapsedMinutesText: String,
    val prefetchSuccessRateText: String,
    val prefetchMeanLatencyMs: Long,
    val keepAliveSuccessRateText: String,
    val keepAliveMeanLatencyMs: Long,
    val releaseSuccessRateText: String,
    val releaseMeanAvailableDeltaKb: Long,
    val meanPssDeltaKb: Long,
    val meanHeapDeltaKb: Long,
) {
    companion object {
        fun from(samples: List<CombinedExperimentSample>): CombinedExperimentSummary {
            val elapsedMs = samples.lastOrNull()?.elapsedMs ?: 0L
            return CombinedExperimentSummary(
                samples = samples.size,
                elapsedMinutesText = String.format(Locale.US, "%.1f min", elapsedMs / 60_000.0),
                prefetchSuccessRateText = successRate(samples.map { it.prefetch }),
                prefetchMeanLatencyMs = mean(samples.filter { it.prefetch.attempted }.map { it.prefetch.latencyMs }),
                keepAliveSuccessRateText = successRate(samples.map { it.keepAlive }),
                keepAliveMeanLatencyMs = mean(samples.map { it.keepAlive.latencyMs }),
                releaseSuccessRateText = successRate(samples.map { it.releaseMemory }),
                releaseMeanAvailableDeltaKb = mean(samples.map { it.releaseMemory.deltaValue }),
                meanPssDeltaKb = mean(samples.map { (it.after.pssKb - it.before.pssKb).toLong() }),
                meanHeapDeltaKb = mean(samples.map { it.after.javaHeapUsedKb - it.before.javaHeapUsedKb }),
            )
        }

        private fun successRate(metrics: List<ActionMetric>): String {
            val attempted = metrics.count { it.attempted }
            if (attempted == 0) return "not run"
            val ok = metrics.count { it.attempted && it.success }
            return String.format(Locale.US, "%.1f%% (%d/%d)", ok * 100.0 / attempted, ok, attempted)
        }

        private fun mean(values: List<Long>): Long =
            if (values.isEmpty()) 0L else ceil(values.average()).toLong()
    }
}
