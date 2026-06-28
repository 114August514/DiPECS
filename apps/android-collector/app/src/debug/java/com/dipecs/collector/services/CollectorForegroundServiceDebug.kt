package com.dipecs.collector.services

import android.content.Intent
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

/**
 * Debug-only dispatch helper for [CollectorForegroundService]. It handles the
 * [ACTION_EXECUTE_AUTHORIZED_ACTION] intent that is only exposed in debug
 * builds via [com.dipecs.collector.addAuthorizedActionCard]. Release builds use
 * the no-op stub so the action constant and dispatch logic are not present in
 * release source code.
 */
object DebugServiceActions {
    const val ACTION_EXECUTE_AUTHORIZED_ACTION = "com.dipecs.collector.action.EXECUTE_AUTHORIZED_ACTION"

    fun handle(service: CollectorForegroundService, intent: Intent?, running: Boolean) {
        when (intent?.action) {
            ACTION_EXECUTE_AUTHORIZED_ACTION -> executeAuthorizedAction(service, intent, running)
        }
    }

    private fun executeAuthorizedAction(
        service: CollectorForegroundService,
        intent: Intent,
        running: Boolean,
    ) {
        val payload = intent.getStringExtra(CollectorForegroundService.EXTRA_AUTHORIZED_ACTION_JSON)
            ?.takeIf { it.isNotBlank() }
            ?: CollectorPreferences.authorizedActionJson(service)
        val shouldStopAfterDispatch = !running

        if (payload.isBlank()) {
            EventRepository.recordInternal(
                service,
                "authorized_action_skipped",
                "No AuthorizedAction JSON configured",
            )
            if (shouldStopAfterDispatch) {
                service.stopSelf()
            }
            return
        }

        runCatching { JSONObject(payload) }
            .onSuccess { json ->
                ActionExecutorBridge.dispatchAuthorizedActionJson(
                    service,
                    json,
                    reason = "service_authorized_action",
                )
            }
            .onFailure { error ->
                EventRepository.recordInternal(
                    service,
                    "authorized_action_rejected",
                    error.message ?: "Invalid AuthorizedAction JSON",
                    JSONObject().put("payload", payload.take(2048)),
                )
            }

        if (shouldStopAfterDispatch) {
            service.stopSelf()
        }
    }
}
