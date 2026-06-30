package com.dipecs.collector.services

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

/**
 * Auto-start the collector foreground service on device boot.
 *
 * Requires `receiver` entry in AndroidManifest.xml with
 * `android.intent.action.BOOT_COMPLETED`.
 * The permission is ignored on normal-app installs; only a system/priv-app
 * will receive the broadcast.
 */
class BootReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return

        EventRepository.recordInternal(
            context,
            "boot_received",
            "System boot completed, starting collector",
            JSONObject(),
        )

        val startIntent = Intent(context, CollectorForegroundService::class.java).apply {
            action = CollectorForegroundService.ACTION_START
        }
        runCatching {
            context.startForegroundService(startIntent)
        }.onFailure { error ->
            EventRepository.recordInternal(
                context,
                "boot_start_failed",
                "Failed to start collector on boot: ${error.message}",
                JSONObject(),
            )
        }
    }
}
