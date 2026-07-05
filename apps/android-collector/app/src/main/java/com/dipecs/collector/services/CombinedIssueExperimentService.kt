package com.dipecs.collector.services

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.IBinder
import com.dipecs.collector.DeviceValidationActivity
import com.dipecs.collector.R
import com.dipecs.collector.validation.CombinedExperimentSnapshot
import com.dipecs.collector.validation.CombinedIssueExperimentRunner
import org.json.JSONObject

class CombinedIssueExperimentService : Service(), CombinedIssueExperimentRunner.Listener {
    private lateinit var runner: CombinedIssueExperimentRunner

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        runner = CombinedIssueExperimentRunner(this, this)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                val duration = intent.getIntExtra(EXTRA_DURATION_MINUTES, 120).coerceAtLeast(1)
                val interval = intent.getIntExtra(EXTRA_INTERVAL_SECONDS, 60).coerceAtLeast(1)
                val target = intent.getStringExtra(EXTRA_PREFETCH_TARGET).orEmpty()
                startForeground(NOTIFICATION_ID, notification("Running combined device experiment"))
                runner.start(duration, interval, target)
                saveSnapshot(runner.snapshot("service started"))
            }
            ACTION_STOP -> {
                runner.stop()
                saveSnapshot(runner.snapshot("stop requested"))
            }
            else -> {
                startForeground(NOTIFICATION_ID, notification("Device experiment ready"))
                saveSnapshot(runner.snapshot("ready"))
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCombinedSnapshot(snapshot: CombinedExperimentSnapshot) {
        saveSnapshot(snapshot)
        val manager = getSystemService(NotificationManager::class.java)
        manager?.notify(
            NOTIFICATION_ID,
            notification("Samples ${snapshot.samples.size}, ${snapshot.summary.prefetchSuccessRateText} prefetch"),
        )
    }

    override fun onCombinedFinished(snapshot: CombinedExperimentSnapshot) {
        saveSnapshot(snapshot.copy(running = false))
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun saveSnapshot(snapshot: CombinedExperimentSnapshot) {
        val json = JSONObject()
            .put("running", snapshot.running)
            .put("started_at_ms", snapshot.startedAtMs)
            .put("elapsed_ms", snapshot.elapsedMs)
            .put("target_duration_ms", snapshot.targetDurationMs)
            .put("interval_ms", snapshot.intervalMs)
            .put("prefetch_target", snapshot.prefetchTarget)
            .put("message", snapshot.message)
            .put("samples", snapshot.samples.size)
            .put("json_path", snapshot.jsonPath)
            .put("markdown_path", snapshot.markdownPath)
            .put("markdown_report", snapshot.markdownReport)
            .put(
                "summary",
                JSONObject()
                    .put("samples", snapshot.summary.samples)
                    .put("elapsed_minutes_text", snapshot.summary.elapsedMinutesText)
                    .put("prefetch_success_rate_text", snapshot.summary.prefetchSuccessRateText)
                    .put("prefetch_mean_latency_ms", snapshot.summary.prefetchMeanLatencyMs)
                    .put("keep_alive_success_rate_text", snapshot.summary.keepAliveSuccessRateText)
                    .put("keep_alive_mean_latency_ms", snapshot.summary.keepAliveMeanLatencyMs)
                    .put("release_success_rate_text", snapshot.summary.releaseSuccessRateText)
                    .put("release_mean_available_delta_kb", snapshot.summary.releaseMeanAvailableDeltaKb)
                    .put("mean_pss_delta_kb", snapshot.summary.meanPssDeltaKb)
                    .put("mean_heap_delta_kb", snapshot.summary.meanHeapDeltaKb),
            )
        getSharedPreferences(PREFS_NAME, MODE_PRIVATE)
            .edit()
            .putString(KEY_SNAPSHOT_JSON, json.toString())
            .apply()
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
            "DiPECS device experiment",
            NotificationManager.IMPORTANCE_LOW,
        )
        channel.description = "Runs long #97/#98/#99 device experiments"
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    companion object {
        const val ACTION_START = "com.dipecs.collector.combined.START"
        const val ACTION_STOP = "com.dipecs.collector.combined.STOP"
        const val EXTRA_DURATION_MINUTES = "duration_minutes"
        const val EXTRA_INTERVAL_SECONDS = "interval_seconds"
        const val EXTRA_PREFETCH_TARGET = "prefetch_target"
        const val PREFS_NAME = "dipecs_combined_issue_experiment"
        const val KEY_SNAPSHOT_JSON = "snapshot_json"

        private const val CHANNEL_ID = "dipecs_combined_issue_experiment"
        private const val NOTIFICATION_ID = 1202
    }
}
