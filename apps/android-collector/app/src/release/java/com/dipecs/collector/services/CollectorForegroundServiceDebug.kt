package com.dipecs.collector.services

import android.content.Intent

/**
 * Release no-op stub for debug-only service actions. The release build does not
 * expose [ACTION_EXECUTE_AUTHORIZED_ACTION]; regular action dispatch only
 * happens through the authenticated localhost socket bridge.
 */
object DebugServiceActions {
    fun handle(service: CollectorForegroundService, intent: Intent?, running: Boolean) {
        // No-op in release source set.
    }
}
