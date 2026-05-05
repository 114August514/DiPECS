package com.dipecs.collector.services

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import com.dipecs.collector.MainActivity
import com.dipecs.collector.R
import com.dipecs.collector.collectors.DeviceContextCollector
import com.dipecs.collector.collectors.UsageCollector
import com.dipecs.collector.model.AndroidRawEventMapper
import com.dipecs.collector.model.CollectorEvent
import com.dipecs.collector.net.CloudUploader
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

class CollectorForegroundService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private lateinit var usageCollector: UsageCollector
    private var running = false

    private val pollRunnable = object : Runnable {
        override fun run() {
            if (!running) {
                return
            }
            usageCollector.collectSinceLastPoll()
            handler.postDelayed(this, USAGE_POLL_INTERVAL_MS)
        }
    }

    private val heartbeatRunnable = object : Runnable {
        override fun run() {
            if (!running) {
                return
            }
            if (!CollectorPreferences.isDeviceContextEnabled(this@CollectorForegroundService)) {
                handler.postDelayed(this, HEARTBEAT_INTERVAL_MS)
                return
            }
            val now = System.currentTimeMillis()
            val deviceContext = DeviceContextCollector.snapshot(this@CollectorForegroundService)
            EventRepository.record(
                this@CollectorForegroundService,
                CollectorEvent(
                    timestampMs = now,
                    source = "device_context",
                    eventType = "context_heartbeat",
                    deviceContext = deviceContext,
                    rawEvent = AndroidRawEventMapper.systemState(now, deviceContext),
                    rawPayload = JSONObject().put("serviceRunning", true),
                ),
            )
            handler.postDelayed(this, HEARTBEAT_INTERVAL_MS)
        }
    }

    private val uploadRunnable = object : Runnable {
        override fun run() {
            if (!running) {
                return
            }
            CloudUploader.uploadRecent(this@CollectorForegroundService, reason = "periodic")
            handler.postDelayed(this, UPLOAD_INTERVAL_MS)
        }
    }

    override fun onCreate() {
        super.onCreate()
        usageCollector = UsageCollector(this)
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action ?: ACTION_START) {
            ACTION_START -> startCollector()
            ACTION_STOP -> stopCollector()
            ACTION_UPLOAD_NOW -> CloudUploader.uploadRecent(this, reason = "manual")
        }
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        handler.removeCallbacksAndMessages(null)
        EventRepository.recordInternal(this, "collector_service_destroyed", "Collector foreground service destroyed")
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startCollector() {
        startForeground(NOTIFICATION_ID, foregroundNotification("Collecting Android action events"))
        if (running) {
            return
        }

        running = true
        EventRepository.recordInternal(this, "collector_service_started", "Collector foreground service started")
        usageCollector.collectSinceLastPoll()
        handler.postDelayed(pollRunnable, USAGE_POLL_INTERVAL_MS)
        handler.post(heartbeatRunnable)
        handler.postDelayed(uploadRunnable, UPLOAD_INTERVAL_MS)
    }

    private fun stopCollector() {
        running = false
        handler.removeCallbacksAndMessages(null)
        EventRepository.recordInternal(this, "collector_service_stopped", "Collector foreground service stopped")
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun foregroundNotification(content: String): Notification {
        val launchIntent = Intent(this, MainActivity::class.java)
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
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Collector foreground service",
            NotificationManager.IMPORTANCE_LOW,
        )
        channel.description = "Keeps DiPECS event collection active"
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    companion object {
        const val ACTION_START = "com.dipecs.collector.action.START"
        const val ACTION_STOP = "com.dipecs.collector.action.STOP"
        const val ACTION_UPLOAD_NOW = "com.dipecs.collector.action.UPLOAD_NOW"

        private const val CHANNEL_ID = "dipecs_collector"
        private const val NOTIFICATION_ID = 1101
        private const val USAGE_POLL_INTERVAL_MS = 5_000L
        private const val HEARTBEAT_INTERVAL_MS = 30_000L
        private const val UPLOAD_INTERVAL_MS = 60_000L
    }
}
