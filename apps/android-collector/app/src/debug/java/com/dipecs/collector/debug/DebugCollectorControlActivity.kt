package com.dipecs.collector.debug

import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import com.dipecs.collector.services.CollectorForegroundService

/**
 * Debug-only adb entrypoint for emulator validation. Launching an activity puts
 * the app in the foreground before starting the collector foreground service.
 */
class DebugCollectorControlActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val serviceIntent = Intent(this, CollectorForegroundService::class.java)
            .setAction(CollectorForegroundService.ACTION_START)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
        finish()
    }
}
