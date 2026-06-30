package com.dipecs.collector.actions

import android.content.Context
import org.json.JSONObject
import com.dipecs.collector.storage.EventRepository

object ActionExecutorBridge {
    const val ACTION_TYPE_PREWARM_PROCESS = "PreWarmProcess"
    const val ACTION_TYPE_PREFETCH_FILE = "PrefetchFile"
    const val ACTION_TYPE_KEEP_ALIVE = "KeepAlive"
    const val ACTION_TYPE_RELEASE_MEMORY = "ReleaseMemory"
    const val ACTION_TYPE_NO_OP = "NoOp"

    fun dispatch(
        context: Context,
        actionType: String,
        target: String?,
        reason: String = "manual",
    ): Boolean {
        val normalizedTarget = target?.trim().takeUnless { it.isNullOrBlank() }
        return when (actionType) {
            ACTION_TYPE_PREWARM_PROCESS -> {
                if (normalizedTarget == null || normalizedTarget.startsWith("own:")) {
                    OwnResourceWarmer.warm(context, normalizedTarget, reason)
                } else {
                    UserVisibleActionNotifier.postLaunchHint(context, normalizedTarget, reason)
                }
            }
            ACTION_TYPE_PREFETCH_FILE -> {
                if (normalizedTarget == null) {
                    EventRepository.recordInternal(
                        context,
                        "action_dispatch_skipped",
                        "PrefetchFile requires a target",
                        JSONObject()
                            .put("actionType", actionType)
                            .put("reason", reason),
                    )
                    false
                } else {
                    AccessibleContentPrefetcher.enqueue(context, normalizedTarget, reason)
                    true
                }
            }
            ACTION_TYPE_KEEP_ALIVE -> {
                ActionMaintenanceScheduler.schedule(context, normalizedTarget, reason)
            }
            ACTION_TYPE_RELEASE_MEMORY -> {
                CacheTrimmer.release(context, normalizedTarget, reason)
                true
            }
            ACTION_TYPE_NO_OP -> {
                EventRepository.recordInternal(
                    context,
                    "action_noop",
                    "NoOp action acknowledged",
                    JSONObject().put("reason", reason),
                )
                true
            }
            else -> {
                EventRepository.recordInternal(
                    context,
                    "action_dispatch_unsupported",
                    "Unsupported action type",
                    JSONObject()
                        .put("actionType", actionType)
                        .put("target", normalizedTarget)
                        .put("reason", reason),
                )
                false
            }
        }
    }

    fun dispatchAuthorizedActionJson(
        context: Context,
        payload: JSONObject,
        reason: String = "authorized_action_json",
    ): Boolean {
        val action = payload.optJSONObject("action")
        if (action == null) {
            EventRepository.recordInternal(
                context,
                "action_dispatch_rejected",
                "AuthorizedAction JSON missing action object",
                JSONObject()
                    .put("reason", reason)
                    .put("payloadBytes", payload.toString().toByteArray(Charsets.UTF_8).size),
            )
            return false
        }

        val actionType = action.optString("action_type").takeIf { it.isNotBlank() }
        val target = action.takeIf { it.has("target") && !it.isNull("target") }?.optString("target")
        if (actionType == null) {
            EventRepository.recordInternal(
                context,
                "action_dispatch_rejected",
                "AuthorizedAction JSON missing action_type",
                JSONObject()
                    .put("reason", reason)
                    .put("payloadBytes", payload.toString().toByteArray(Charsets.UTF_8).size),
            )
            return false
        }

        return dispatch(context, actionType, target, reason)
    }
}
