package com.dipecs.collector

import android.Manifest
import android.content.Context
import android.content.Intent
import android.os.Build
import com.dipecs.collector.services.CollectorForegroundService

internal fun Context.startCollectorService(
    action: String,
    prefetchTarget: String? = null,
    authorizedActionJson: String? = null,
) {
    val intent = Intent(this, CollectorForegroundService::class.java).setAction(action)
    if (!prefetchTarget.isNullOrBlank()) {
        intent.putExtra(CollectorForegroundService.EXTRA_PREFETCH_TARGET, prefetchTarget)
    }
    if (!authorizedActionJson.isNullOrBlank()) {
        intent.putExtra(CollectorForegroundService.EXTRA_AUTHORIZED_ACTION_JSON, authorizedActionJson)
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && action == CollectorForegroundService.ACTION_START) {
        startForegroundService(intent)
    } else {
        startService(intent)
    }
}

internal fun Context.requestNotificationPermission(activity: android.app.Activity, requestCode: Int) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), requestCode)
    } else {
        toast("此 Android 版本无需通知运行时权限")
    }
}
