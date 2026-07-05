package com.dipecs.collector.validation

import android.content.Context
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import com.dipecs.collector.storage.EventStore
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean

class DeviceValidationRunner(
    context: Context,
    private val listener: Listener,
) {
    interface Listener {
        fun onSnapshot(snapshot: DeviceValidationSnapshot)
        fun onFinished(snapshot: DeviceValidationSnapshot)
    }

    private val appContext = context.applicationContext
    private val cancelRequested = AtomicBoolean(false)
    private val results = linkedMapOf<String, ValidationItemResult>()
    private var worker: Thread? = null

    fun start(plan: DeviceValidationPlan) {
        if (worker?.isAlive == true) return
        cancelRequested.set(false)
        results.clear()
        plan.selectedItems().forEach { item ->
            results[item.id] = ValidationItemResult(item.id, item.title, ValidationStatus.Pending)
        }
        publish("准备执行本地真机验证")

        worker = Thread {
            runCatching {
                enforceLocalOnlyMode()
                for (item in plan.selectedItems()) {
                    if (cancelRequested.get()) {
                        markSkipped(item, "用户已停止，剩余测试跳过")
                        continue
                    }
                    runItem(item, plan)
                }
                exportResults()
            }.onFailure { error ->
                publish("验证执行异常：${error.message ?: error.javaClass.simpleName}")
            }
            listener.onFinished(snapshot())
        }.apply {
            name = "dipecs-device-validation"
            start()
        }
    }

    fun stop() {
        cancelRequested.set(true)
        publish("已请求停止；当前步骤会尽快收尾")
    }

    private fun runItem(item: ValidationItem, plan: DeviceValidationPlan) {
        update(item, ValidationStatus.Running, "运行中", emptyMap())
        val startedAt = System.currentTimeMillis()
        val result = when (item) {
            ValidationItem.BasicRun -> runBasicRun(startedAt)
            ValidationItem.PrefetchFile -> runPrefetch(startedAt, plan.prefetchTarget)
            ValidationItem.KeepAlive -> runKeepAlive(startedAt)
            ValidationItem.ReleaseMemory -> runReleaseMemory(startedAt)
        }
        results[item.id] = result
        publish("${item.title}：${result.status.label} - ${result.message}")
    }

    private fun runBasicRun(startedAt: Long): ValidationItemResult {
        val stats = EventStore(appContext).stats()
        val uploadMode = CollectorPreferences.uploadMode(appContext)
        val uploadEnabled = CollectorPreferences.isUploadEnabled(appContext)
        val ok = uploadMode == CollectorPreferences.MODE_MOCK && !uploadEnabled
        return ValidationItemResult(
            id = ValidationItem.BasicRun.id,
            title = ValidationItem.BasicRun.title,
            status = if (ok) ValidationStatus.Passed else ValidationStatus.Failed,
            message = if (ok) "本地模式已锁定；不会使用云端 LLM 或 API key"
                else "本地模式未锁定：mode=$uploadMode upload=$uploadEnabled",
            durationMs = elapsed(startedAt),
            metrics = linkedMapOf(
                "collector_running" to CollectorPreferences.isCollectorRunning(appContext).toString(),
                "action_socket_listening" to CollectorPreferences.isActionSocketListening(appContext).toString(),
                "upload_mode" to uploadMode,
                "upload_enabled" to uploadEnabled.toString(),
                "trace_rows" to stats.totalRows.toString(),
                "raw_event_rows" to stats.rawEventRows.toString(),
            ),
        )
    }

    private fun runPrefetch(startedAt: Long, target: String): ValidationItemResult {
        val normalized = target.trim()
        if (normalized.isBlank()) {
            return skipped(ValidationItem.PrefetchFile, startedAt, "未填写 HTTPS / content URI 目标，PrefetchFile 跳过")
        }
        val before = cacheStats()
        val result = ActionExecutorBridge.dispatch(
            appContext,
            ActionExecutorBridge.ACTION_TYPE_PREFETCH_FILE,
            normalized,
            reason = "device_validation_97",
        )
        waitUntil(12_000L) {
            cancelRequested.get() || EventStore(appContext).readRecent(48).any {
                val type = it.optString("eventType")
                type == "prefetch_succeeded" || type == "prefetch_failed" || type == "prefetch_rejected"
            }
        }
        val after = cacheStats()
        val recent = EventStore(appContext).readRecent(48)
        val succeeded = recent.any { it.optString("eventType") == "prefetch_succeeded" }
        val failed = recent.lastOrNull {
            val type = it.optString("eventType")
            type == "prefetch_failed" || type == "prefetch_rejected"
        }
        val passed = result.success && succeeded && after.bytes >= before.bytes
        return ValidationItemResult(
            id = ValidationItem.PrefetchFile.id,
            title = ValidationItem.PrefetchFile.title,
            status = when {
                cancelRequested.get() -> ValidationStatus.Stopped
                passed -> ValidationStatus.Passed
                else -> ValidationStatus.Failed
            },
            message = when {
                cancelRequested.get() -> "用户停止 PrefetchFile 测试"
                passed -> "PrefetchFile 已进入生产路径并写入缓存"
                failed != null -> failed.optString("text", "PrefetchFile 失败")
                else -> "未在等待窗口内看到 prefetch_succeeded"
            },
            durationMs = elapsed(startedAt),
            metrics = linkedMapOf(
                "target" to normalized.take(96),
                "dispatch_summary" to result.summary,
                "cache_files_before" to before.files.toString(),
                "cache_files_after" to after.files.toString(),
                "cache_bytes_before" to before.bytes.toString(),
                "cache_bytes_after" to after.bytes.toString(),
            ),
        )
    }

    private fun runKeepAlive(startedAt: Long): ValidationItemResult {
        val beforeHeartbeat = CollectorPreferences.lastHeartbeatMs(appContext)
        val result = ActionExecutorBridge.dispatch(
            appContext,
            ActionExecutorBridge.ACTION_TYPE_KEEP_ALIVE,
            "work:collector_heartbeat",
            reason = "device_validation_98",
        )
        waitUntil(7_000L) {
            cancelRequested.get() ||
                CollectorPreferences.lastHeartbeatMs(appContext) > beforeHeartbeat ||
                EventStore(appContext).readRecent(48).any { it.optString("eventType") == "keep_alive_scheduled" }
        }
        val afterHeartbeat = CollectorPreferences.lastHeartbeatMs(appContext)
        val scheduled = EventStore(appContext).readRecent(64).any {
            it.optString("eventType") == "keep_alive_scheduled" ||
                it.optString("eventType") == "keep_alive_system" ||
                it.optString("eventType") == "keep_alive_fallback"
        }
        val executed = afterHeartbeat > beforeHeartbeat
        return ValidationItemResult(
            id = ValidationItem.KeepAlive.id,
            title = ValidationItem.KeepAlive.title,
            status = when {
                cancelRequested.get() -> ValidationStatus.Stopped
                scheduled || executed -> ValidationStatus.Passed
                else -> ValidationStatus.Failed
            },
            message = when {
                cancelRequested.get() -> "用户停止 KeepAlive 测试"
                executed -> "维护任务已执行并刷新 heartbeat"
                scheduled -> "KeepAlive 已调度；系统可能稍后执行 JobScheduler"
                else -> "未观察到 KeepAlive 调度或 heartbeat"
            },
            durationMs = elapsed(startedAt),
            metrics = linkedMapOf(
                "dispatch_summary" to result.summary,
                "dispatch_success" to result.success.toString(),
                "heartbeat_before" to beforeHeartbeat.toString(),
                "heartbeat_after" to afterHeartbeat.toString(),
                "job_scheduled_or_system" to scheduled.toString(),
            ),
        )
    }

    private fun runReleaseMemory(startedAt: Long): ValidationItemResult {
        val prefetchDir = File(appContext.cacheDir, "prefetch")
        prefetchDir.mkdirs()
        repeat(4) { index ->
            File(prefetchDir, "validation-$index.bin").writeBytes(ByteArray(32 * 1024) { index.toByte() })
        }
        val before = cacheStats()
        val result = ActionExecutorBridge.dispatch(
            appContext,
            ActionExecutorBridge.ACTION_TYPE_RELEASE_MEMORY,
            "cache:prefetch",
            reason = "device_validation_99",
        )
        val after = cacheStats()
        val released = after.bytes < before.bytes && after.files < before.files
        return ValidationItemResult(
            id = ValidationItem.ReleaseMemory.id,
            title = ValidationItem.ReleaseMemory.title,
            status = when {
                cancelRequested.get() -> ValidationStatus.Stopped
                result.success && released -> ValidationStatus.Passed
                else -> ValidationStatus.Failed
            },
            message = when {
                cancelRequested.get() -> "用户停止 ReleaseMemory 测试"
                result.success && released -> "ReleaseMemory 清理了 DiPECS 预取缓存"
                else -> "ReleaseMemory 未观察到缓存下降"
            },
            durationMs = elapsed(startedAt),
            metrics = linkedMapOf(
                "dispatch_summary" to result.summary,
                "cache_files_before" to before.files.toString(),
                "cache_files_after" to after.files.toString(),
                "cache_bytes_before" to before.bytes.toString(),
                "cache_bytes_after" to after.bytes.toString(),
            ),
        )
    }

    private fun enforceLocalOnlyMode() {
        CollectorPreferences.setUploadMode(appContext, CollectorPreferences.MODE_MOCK)
        CollectorPreferences.setUploadEnabled(appContext, false)
        CollectorPreferences.setApiKey(appContext, "")
        EventRepository.recordInternal(
            appContext,
            "device_validation_local_only",
            "Real-device validation locked to local-only mode; cloud LLM is excluded",
            JSONObject()
                .put("uploadMode", CollectorPreferences.uploadMode(appContext))
                .put("uploadEnabled", CollectorPreferences.isUploadEnabled(appContext)),
        )
    }

    private fun markSkipped(item: ValidationItem, message: String) {
        results[item.id] = ValidationItemResult(item.id, item.title, ValidationStatus.Skipped, message)
        publish("${item.title}：已跳过")
    }

    private fun skipped(item: ValidationItem, startedAt: Long, message: String): ValidationItemResult =
        ValidationItemResult(item.id, item.title, ValidationStatus.Skipped, message, elapsed(startedAt))

    private fun update(item: ValidationItem, status: ValidationStatus, message: String, metrics: Map<String, String>) {
        results[item.id] = ValidationItemResult(item.id, item.title, status, message, metrics = metrics)
        publish("${item.title}：${status.label}")
    }

    private fun publish(message: String) {
        listener.onSnapshot(snapshot(message))
    }

    private fun snapshot(message: String? = null): DeviceValidationSnapshot =
        DeviceValidationSnapshot(
            running = worker?.isAlive == true && !cancelRequested.get(),
            localOnly = CollectorPreferences.uploadMode(appContext) == CollectorPreferences.MODE_MOCK &&
                !CollectorPreferences.isUploadEnabled(appContext),
            message = message ?: "",
            results = results.values.toList(),
        )

    private fun exportResults() {
        val outDir = File(appContext.getExternalFilesDir(null) ?: appContext.filesDir, "validation")
        outDir.mkdirs()
        val ts = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val output = File(outDir, "device-validation-$ts.jsonl")
        output.bufferedWriter().use { writer ->
            results.values.forEach { result ->
                writer.write(result.toJson().toString())
                writer.newLine()
            }
        }
        EventRepository.recordInternal(
            appContext,
            "device_validation_exported",
            "Device validation result exported",
            JSONObject().put("path", output.absolutePath).put("items", results.size),
        )
        publish("结果已导出：${output.absolutePath}")
    }

    private fun waitUntil(timeoutMs: Long, predicate: () -> Boolean) {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            if (predicate()) return
            Thread.sleep(250L)
        }
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

    private fun elapsed(startedAt: Long): Long = System.currentTimeMillis() - startedAt

    private data class CacheStats(val files: Int, val bytes: Long)
}

data class DeviceValidationPlan(
    val runBasic: Boolean,
    val runPrefetch: Boolean,
    val runKeepAlive: Boolean,
    val runReleaseMemory: Boolean,
    val prefetchTarget: String,
) {
    fun selectedItems(): List<ValidationItem> =
        buildList {
            if (runBasic) add(ValidationItem.BasicRun)
            if (runPrefetch) add(ValidationItem.PrefetchFile)
            if (runKeepAlive) add(ValidationItem.KeepAlive)
            if (runReleaseMemory) add(ValidationItem.ReleaseMemory)
        }
}

data class DeviceValidationSnapshot(
    val running: Boolean,
    val localOnly: Boolean,
    val message: String,
    val results: List<ValidationItemResult>,
)

data class ValidationItemResult(
    val id: String,
    val title: String,
    val status: ValidationStatus,
    val message: String = "",
    val durationMs: Long = 0,
    val metrics: Map<String, String> = emptyMap(),
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("id", id)
            .put("title", title)
            .put("status", status.name.lowercase(Locale.US))
            .put("message", message)
            .put("duration_ms", durationMs)
            .put("metrics", JSONObject(metrics))
}

enum class ValidationStatus(val label: String) {
    Pending("等待"),
    Running("运行中"),
    Passed("通过"),
    Failed("失败"),
    Skipped("跳过"),
    Stopped("已停止"),
}

sealed class ValidationItem(val id: String, val title: String) {
    object BasicRun : ValidationItem("basic_run", "项目基础跑通")
    object PrefetchFile : ValidationItem("issue_97_prefetch_file", "#97 PrefetchFile")
    object KeepAlive : ValidationItem("issue_98_keep_alive", "#98 KeepAlive")
    object ReleaseMemory : ValidationItem("issue_99_release_memory", "#99 ReleaseMemory")
}
