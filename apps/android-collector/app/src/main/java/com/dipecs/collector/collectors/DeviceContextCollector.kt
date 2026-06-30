package com.dipecs.collector.collectors

import android.app.NotificationManager
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioDeviceInfo
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
        val bluetoothManager = appContext.getSystemService(BluetoothManager::class.java)
        val notificationManager = appContext.getSystemService(NotificationManager::class.java)
        val powerManager = appContext.getSystemService(PowerManager::class.java)
        val headphoneConnected = audioManager?.hasConnectedHeadphone() ?: false

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
            locationType = "Unknown",
            headphoneConnected = headphoneConnected,
            bluetoothConnected = bluetoothManager?.hasConnectedBluetoothDevice() ?: headphoneConnected,
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

    private fun AudioManager.hasConnectedHeadphone(): Boolean =
        getDevices(AudioManager.GET_DEVICES_OUTPUTS).any { device ->
            when (device.type) {
                AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
                AudioDeviceInfo.TYPE_WIRED_HEADSET,
                AudioDeviceInfo.TYPE_USB_HEADSET,
                AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
                AudioDeviceInfo.TYPE_BLUETOOTH_SCO
                -> true
                else -> false
            }
        }

    private fun BluetoothManager.hasConnectedBluetoothDevice(): Boolean =
        runCatching {
            val adapter = adapter ?: return@runCatching false
            listOf(
                BluetoothProfile.A2DP,
                BluetoothProfile.HEADSET,
                BluetoothProfile.GATT,
            ).any { profile ->
                adapter.getProfileConnectionState(profile) == BluetoothProfile.STATE_CONNECTED
            }
        }.getOrDefault(false)
}
