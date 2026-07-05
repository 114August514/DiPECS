package com.dipecs.collector.services

import android.app.ActivityManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Debug
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Process
import com.dipecs.collector.DeviceValidationActivity
import com.dipecs.collector.R
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class PerformanceExperimentService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private var running = false
    private var mode = MODE_BASELINE
    private var startedAtMs = 0L
    private var samples = 0
    private var outputFile: File? = null
    private var baselineSample: JSONObject? = null
    private var lastSample: JSONObject? = null
    private var actionTick = 0

    private val sampleRunnable = object : Runnable {
        override fun run() {
            if (!running) return
            val sample = collectSample()
            if (baselineSample == null) baselineSample = sample
            lastSample = sample
            samples += 1
            outputFile?.appendText(sample.toString() + "\n")
            if (mode == MODE_ACTION_LOOP) {
                maybeRunActionTick()
            }
            updateState("running")
            handler.postDelayed(this, SAMPLE_INTERVAL_MS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> startExperiment(intent.getStringExtra(EXTRA_MODE) ?: MODE_BASELINE)
            ACTION_STOP -> stopExperiment()
            else -> startExperiment(MODE_BASELINE)
        }
        return START_STICKY
    }

    override fun onDestroy() {
        if (running) {
            stopExperiment()
        }
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startExperiment(requestedMode: String) {
        if (running) {
            return
        }
        mode = normalizeMode(requestedMode)
        running = true
        startedAtMs = System.currentTimeMillis()
        samples = 0
        baselineSample = null
        lastSample = null
        actionTick = 0
        outputFile = newOutputFile("jsonl")
        startForeground(NOTIFICATION_ID, notification("性能实验运行中：$mode"))

        when (mode) {
            MODE_BASELINE -> {
                startService(Intent(this, CollectorForegroundService::class.java).setAction(CollectorForegroundService.ACTION_STOP))
            }
            MODE_OBSERVE -> {
                CollectorPreferences.setUploadMode(this, CollectorPreferences.MODE_MOCK)
                CollectorPreferences.setUploadEnabled(this, false)
                startService(Intent(this, CollectorForegroundService::class.java).setAction(CollectorForegroundService.ACTION_START))
            }
            MODE_ACTION_LOOP -> {
                CollectorPreferences.setUploadMode(this, CollectorPreferences.MODE_MOCK)
                CollectorPreferences.setUploadEnabled(this, false)
                startService(Intent(this, CollectorForegroundService::class.java).setAction(CollectorForegroundService.ACTION_START))
            }
        }

        EventRepository.recordInternal(
            this,
            "performance_experiment_started",
            "Started performance experiment",
            JSONObject().put("mode", mode).put("output", outputFile?.absolutePath),
        )
        updateState("running")
        handler.post(sampleRunnable)
    }

    private fun stopExperiment() {
        if (!running) {
            stopSelf()
            return
        }
        running = false
        handler.removeCallbacks(sampleRunnable)
        val finalSample = collectSample()
        outputFile?.appendText(finalSample.toString() + "\n")
        lastSample = finalSample
        val summary = writeSummary()
        EventRepository.recordInternal(
            this,
            "performance_experiment_stopped",
            "Stopped performance experiment",
            JSONObject().put("mode", mode).put("summary", summary.absolutePath),
        )
        updateState("stopped", summary.absolutePath)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun collectSample(): JSONObject {
        val runtime = Runtime.getRuntime()
        val memoryInfo = Debug.MemoryInfo()
        Debug.getMemoryInfo(memoryInfo)
        val activityManager = getSystemService(ActivityManager::class.java)
        val pssKb = activityManager
            ?.getProcessMemoryInfo(intArrayOf(Process.myPid()))
            ?.firstOrNull()
            ?.totalPss
            ?: memoryInfo.totalPss
        val battery = registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val level = battery?.getIntExtra("level", -1) ?: -1
        val scale = battery?.getIntExtra("scale", -1) ?: -1
        val tempTenths = battery?.getIntExtra("temperature", Int.MIN_VALUE) ?: Int.MIN_VALUE
        val voltageMv = battery?.getIntExtra("voltage", -1) ?: -1
        val batteryPct = if (level >= 0 && scale > 0) level * 100.0 / scale else null
        val tempC = if (tempTenths != Int.MIN_VALUE) tempTenths / 10.0 else null
        return JSONObject()
            .put("schema_version", "dipecs.performance_sample.v1")
            .put("mode", mode)
            .put("timestamp_ms", System.currentTimeMillis())
            .put("elapsed_ms", System.currentTimeMillis() - startedAtMs)
            .put("sample_index", samples)
            .put("pss_kb", pssKb)
            .put("java_heap_used_kb", (runtime.totalMemory() - runtime.freeMemory()) / 1024)
            .put("java_heap_max_kb", runtime.maxMemory() / 1024)
            .put("battery_pct", batteryPct ?: JSONObject.NULL)
            .put("battery_temp_c", tempC ?: JSONObject.NULL)
            .put("battery_voltage_mv", if (voltageMv > 0) voltageMv else JSONObject.NULL)
            .put("collector_running", CollectorPreferences.isCollectorRunning(this))
            .put("action_socket_listening", CollectorPreferences.isActionSocketListening(this))
    }

    private fun maybeRunActionTick() {
        actionTick += 1
        if (actionTick % ACTION_INTERVAL_SAMPLES != 0) return
        ActionExecutorBridge.dispatch(
            this,
            ActionExecutorBridge.ACTION_TYPE_KEEP_ALIVE,
            "work:collector_heartbeat",
            reason = "performance_action_loop",
        )
        ActionExecutorBridge.dispatch(
            this,
            ActionExecutorBridge.ACTION_TYPE_RELEASE_MEMORY,
            "cache:prefetch",
            reason = "performance_action_loop",
        )
    }

    private fun writeSummary(): File {
        val summary = newOutputFile("summary.json")
        val baseline = baselineSample
        val latest = lastSample
        val json = JSONObject()
            .put("schema_version", "dipecs.performance_experiment.v1")
            .put("mode", mode)
            .put("started_at_ms", startedAtMs)
            .put("ended_at_ms", System.currentTimeMillis())
            .put("duration_ms", System.currentTimeMillis() - startedAtMs)
            .put("samples", samples)
            .put("sample_file", outputFile?.absolutePath ?: JSONObject.NULL)
            .put("baseline", baseline ?: JSONObject.NULL)
            .put("latest", latest ?: JSONObject.NULL)
            .put("deltas", deltas(baseline, latest))
        summary.writeText(json.toString(2))
        return summary
    }

    private fun deltas(baseline: JSONObject?, latest: JSONObject?): JSONObject {
        if (baseline == null || latest == null) return JSONObject()
        return JSONObject()
            .put("pss_kb", latest.optLong("pss_kb") - baseline.optLong("pss_kb"))
            .put("java_heap_used_kb", latest.optLong("java_heap_used_kb") - baseline.optLong("java_heap_used_kb"))
            .put("battery_pct", optDoubleDelta(baseline, latest, "battery_pct"))
            .put("battery_temp_c", optDoubleDelta(baseline, latest, "battery_temp_c"))
    }

    private fun optDoubleDelta(a: JSONObject, b: JSONObject, key: String): Any {
        if (a.isNull(key) || b.isNull(key)) return JSONObject.NULL
        return b.optDouble(key) - a.optDouble(key)
    }

    private fun updateState(status: String, summaryPath: String = "") {
        val prefs = getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
        prefs.edit()
            .putString(KEY_STATUS, status)
            .putString(KEY_MODE, mode)
            .putLong(KEY_STARTED_AT_MS, startedAtMs)
            .putInt(KEY_SAMPLES, samples)
            .putString(KEY_SAMPLE_PATH, outputFile?.absolutePath ?: "")
            .putString(KEY_SUMMARY_PATH, summaryPath)
            .putString(KEY_LATEST_SAMPLE, lastSample?.toString() ?: "")
            .apply()
    }

    private fun newOutputFile(suffix: String): File {
        val outDir = File(getExternalFilesDir(null) ?: filesDir, "performance")
        outDir.mkdirs()
        val ts = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        return File(outDir, "performance-$mode-$ts.$suffix")
    }

    private fun notification(content: String): Notification {
        val launchIntent = Intent(this, DeviceValidationActivity::class.java)
        val pendingIntent = PendingIntent.getActivity(
            this,
            0,
            launchIntent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        return builder
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(content)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .build()
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "DiPECS performance experiment",
            NotificationManager.IMPORTANCE_LOW,
        )
        channel.description = "Runs long performance experiments for DiPECS"
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    private fun normalizeMode(value: String): String =
        when (value) {
            MODE_OBSERVE, MODE_ACTION_LOOP -> value
            else -> MODE_BASELINE
        }

    companion object {
        const val ACTION_START = "com.dipecs.collector.performance.START"
        const val ACTION_STOP = "com.dipecs.collector.performance.STOP"
        const val EXTRA_MODE = "mode"
        const val MODE_BASELINE = "baseline_idle"
        const val MODE_OBSERVE = "dipecs_observe_only"
        const val MODE_ACTION_LOOP = "dipecs_action_loop"

        const val PREFS_NAME = "dipecs_performance_experiment"
        const val KEY_STATUS = "status"
        const val KEY_MODE = "mode"
        const val KEY_STARTED_AT_MS = "started_at_ms"
        const val KEY_SAMPLES = "samples"
        const val KEY_SAMPLE_PATH = "sample_path"
        const val KEY_SUMMARY_PATH = "summary_path"
        const val KEY_LATEST_SAMPLE = "latest_sample"

        private const val CHANNEL_ID = "dipecs_performance_experiment"
        private const val NOTIFICATION_ID = 1201
        private const val SAMPLE_INTERVAL_MS = 60_000L
        private const val ACTION_INTERVAL_SAMPLES = 10
    }
}
