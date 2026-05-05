package com.dipecs.collector.collectors

import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.PowerManager
import com.dipecs.collector.model.DeviceContext
import java.util.TimeZone

object DeviceContextCollector {
    fun snapshot(context: Context): DeviceContext {
        val appContext = context.applicationContext
        val batteryIntent = appContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED))
        val batteryLevel = batteryIntent?.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) ?: -1
        val batteryScale = batteryIntent?.getIntExtra(BatteryManager.EXTRA_SCALE, -1) ?: -1
        val batteryPercent = if (batteryLevel >= 0 && batteryScale > 0) {
            ((batteryLevel * 100f) / batteryScale).toInt()
        } else {
            null
        }
        val batteryStatus = batteryIntent?.getIntExtra(BatteryManager.EXTRA_STATUS, -1) ?: -1
        val isCharging = when (batteryStatus) {
            BatteryManager.BATTERY_STATUS_CHARGING,
            BatteryManager.BATTERY_STATUS_FULL -> true
            BatteryManager.BATTERY_STATUS_DISCHARGING,
            BatteryManager.BATTERY_STATUS_NOT_CHARGING -> false
            else -> null
        }

        val audioManager = appContext.getSystemService(AudioManager::class.java)
        val notificationManager = appContext.getSystemService(NotificationManager::class.java)
        val powerManager = appContext.getSystemService(PowerManager::class.java)

        return DeviceContext(
            timezone = TimeZone.getDefault().id,
            batteryPercent = batteryPercent,
            isCharging = isCharging,
            networkType = networkType(appContext),
            isScreenOn = powerManager?.isInteractive ?: false,
            ringerMode = when (audioManager?.ringerMode) {
                AudioManager.RINGER_MODE_NORMAL -> "normal"
                AudioManager.RINGER_MODE_VIBRATE -> "vibrate"
                AudioManager.RINGER_MODE_SILENT -> "silent"
                else -> "unknown"
            },
            doNotDisturbMode = runCatching { notificationManager?.currentInterruptionFilter }.getOrNull(),
        )
    }

    private fun networkType(context: Context): String {
        val connectivityManager = context.getSystemService(ConnectivityManager::class.java) ?: return "unknown"
        val network = connectivityManager.activeNetwork ?: return "offline"
        val capabilities = connectivityManager.getNetworkCapabilities(network) ?: return "unknown"
        return when {
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_BLUETOOTH) -> "bluetooth"
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN) -> "vpn"
            else -> "other"
        }
    }
}
