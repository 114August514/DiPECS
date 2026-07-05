package com.dipecs.collector.validation

import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Debug
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.view.Choreographer
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class DeviceImpactMonitor(
    context: Context,
    private val listener: (ImpactSnapshot) -> Unit,
) {
    private val appContext = context.applicationContext
    private val handler = Handler(Looper.getMainLooper())
    private val choreographer = Choreographer.getInstance()
    private var running = false
    private var startedAtMs = 0L
    private var baseline: ImpactSample? = null
    private var latest: ImpactSample? = null
    private var frameCount = 0
    private var jankFrames = 0
    private var lastFrameNanos = 0L

    private val sampler = object : Runnable {
        override fun run() {
            if (!running) return
            latest = sample()
            publish()
            handler.postDelayed(this, SAMPLE_INTERVAL_MS)
        }
    }

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!running) return
            if (lastFrameNanos > 0L) {
                val deltaMs = (frameTimeNanos - lastFrameNanos) / 1_000_000.0
                frameCount += 1
                if (deltaMs > JANK_FRAME_MS) {
                    jankFrames += 1
                }
            }
            lastFrameNanos = frameTimeNanos
            choreographer.postFrameCallback(this)
        }
    }

    fun start() {
        if (running) return
        running = true
        startedAtMs = System.currentTimeMillis()
        frameCount = 0
        jankFrames = 0
        lastFrameNanos = 0L
        baseline = sample()
        latest = baseline
        EventRepository.recordInternal(
            appContext,
            "device_impact_monitor_started",
            "Started local device impact monitoring",
        )
        choreographer.postFrameCallback(frameCallback)
        handler.post(sampler)
        publish()
    }

    fun stop(): File? {
        if (!running) return null
        running = false
        choreographer.removeFrameCallback(frameCallback)
        handler.removeCallbacks(sampler)
        latest = sample()
        val output = export()
        EventRepository.recordInternal(
            appContext,
            "device_impact_monitor_stopped",
            "Stopped local device impact monitoring",
            JSONObject().put("path", output.absolutePath),
        )
        publish()
        return output
    }

    fun snapshot(): ImpactSnapshot =
        ImpactSnapshot(
            running = running,
            elapsedMs = if (startedAtMs > 0L) System.currentTimeMillis() - startedAtMs else 0L,
            baseline = baseline,
            latest = latest,
            frameCount = frameCount,
            jankFrames = jankFrames,
        )

    private fun publish() {
        listener(snapshot())
    }

    private fun sample(): ImpactSample {
        val memoryInfo = Debug.MemoryInfo()
        Debug.getMemoryInfo(memoryInfo)
        val runtime = Runtime.getRuntime()
        val battery = appContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = battery?.getIntExtra("level", -1) ?: -1
        val scale = battery?.getIntExtra("scale", -1) ?: -1
        val tempTenths = battery?.getIntExtra("temperature", Int.MIN_VALUE) ?: Int.MIN_VALUE
        val voltageMv = battery?.getIntExtra("voltage", -1) ?: -1
        val batteryPct = if (level >= 0 && scale > 0) level * 100.0 / scale else null
        val tempC = if (tempTenths != Int.MIN_VALUE) tempTenths / 10.0 else null
        val activityManager = appContext.getSystemService(ActivityManager::class.java)
        val pssKb = activityManager
            ?.getProcessMemoryInfo(intArrayOf(Process.myPid()))
            ?.firstOrNull()
            ?.totalPss
            ?: memoryInfo.totalPss
        return ImpactSample(
            timestampMs = System.currentTimeMillis(),
            pssKb = pssKb,
            javaHeapUsedKb = (runtime.totalMemory() - runtime.freeMemory()) / 1024,
            javaHeapMaxKb = runtime.maxMemory() / 1024,
            batteryPct = batteryPct,
            batteryTempC = tempC,
            batteryVoltageMv = voltageMv.takeIf { it > 0 },
        )
    }

    private fun export(): File {
        val outDir = File(appContext.getExternalFilesDir(null) ?: appContext.filesDir, "validation")
        outDir.mkdirs()
        val ts = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val output = File(outDir, "device-impact-$ts.json")
        val snap = snapshot()
        val json = JSONObject()
            .put("schema_version", "dipecs.device_impact.v1")
            .put("started_at_ms", startedAtMs)
            .put("elapsed_ms", snap.elapsedMs)
            .put("frame_count", frameCount)
            .put("jank_frames", jankFrames)
            .put("jank_pct", snap.jankPct)
            .put("baseline", baseline?.toJson() ?: JSONObject.NULL)
            .put("latest", latest?.toJson() ?: JSONObject.NULL)
            .put("deltas", snap.deltasJson())
        output.writeText(json.toString(2))
        return output
    }

    companion object {
        private const val SAMPLE_INTERVAL_MS = 1_000L
        private const val JANK_FRAME_MS = 32.0
    }
}

data class ImpactSnapshot(
    val running: Boolean,
    val elapsedMs: Long,
    val baseline: ImpactSample?,
    val latest: ImpactSample?,
    val frameCount: Int,
    val jankFrames: Int,
) {
    val jankPct: Double
        get() = if (frameCount > 0) jankFrames * 100.0 / frameCount else 0.0

    fun deltasJson(): JSONObject =
        JSONObject()
            .put("pss_kb", deltaLong { pssKb })
            .put("java_heap_used_kb", deltaLong { javaHeapUsedKb })
            .put("battery_pct", deltaDouble { batteryPct })
            .put("battery_temp_c", deltaDouble { batteryTempC })
}

data class ImpactSample(
    val timestampMs: Long,
    val pssKb: Int,
    val javaHeapUsedKb: Long,
    val javaHeapMaxKb: Long,
    val batteryPct: Double?,
    val batteryTempC: Double?,
    val batteryVoltageMv: Int?,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("timestamp_ms", timestampMs)
            .put("pss_kb", pssKb)
            .put("java_heap_used_kb", javaHeapUsedKb)
            .put("java_heap_max_kb", javaHeapMaxKb)
            .put("battery_pct", batteryPct ?: JSONObject.NULL)
            .put("battery_temp_c", batteryTempC ?: JSONObject.NULL)
            .put("battery_voltage_mv", batteryVoltageMv ?: JSONObject.NULL)
}

private fun ImpactSnapshot.deltaLong(selector: ImpactSample.() -> Number): Long? {
    val base = baseline ?: return null
    val now = latest ?: return null
    return now.selector().toLong() - base.selector().toLong()
}

private fun ImpactSnapshot.deltaDouble(selector: ImpactSample.() -> Double?): Double? {
    val base = baseline?.selector() ?: return null
    val now = latest?.selector() ?: return null
    return now - base
}
