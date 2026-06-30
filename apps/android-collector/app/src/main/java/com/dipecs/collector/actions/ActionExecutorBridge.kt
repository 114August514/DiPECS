package com.dipecs.collector.actions

import android.content.Context
import org.json.JSONObject
import com.dipecs.collector.storage.EventRepository

/**
 * Bridge that routes authorized actions to the correct executor.
 *
 * v2 change: `dispatch()` now returns `SystemActionExecutors.ActionResult`
 * with latency and was-success, so callers can produce honest
 * `BridgeExecuteResponse` back to the Rust side.
 */
object ActionExecutorBridge {
    const val ACTION_TYPE_PREWARM_PROCESS = "PreWarmProcess"
    const val ACTION_TYPE_PREFETCH_FILE = "PrefetchFile"
    const val ACTION_TYPE_KEEP_ALIVE = "KeepAlive"
    const val ACTION_TYPE_RELEASE_MEMORY = "ReleaseMemory"
    const val ACTION_TYPE_NO_OP = "NoOp"

    /**
     * Dispatch a single action with structured result.
     */
    fun dispatch(
        context: Context,
        actionType: String,
        target: String?,
        reason: String = "manual",
    ): SystemActionExecutors.ActionResult {
        return when (actionType) {
            ACTION_TYPE_PREWARM_PROCESS -> {
                val t = target ?: "own:resources"
                SystemActionExecutors.prewarmProcess(context, t, reason)
            }
            ACTION_TYPE_PREFETCH_FILE -> {
                if (target.isNullOrBlank()) {
                    val err = SystemActionExecutors.ActionResult(
                        success = false,
                        summary = "prefetch_skipped",
                        latencyUs = 0,
                        error = "PrefetchFile requires a target",
                    )
                    EventRepository.recordInternal(
                        context,
                        "action_dispatch_skipped",
                        "PrefetchFile requires a target",
                        JSONObject().put("actionType", actionType).put("reason", reason),
                    )
                    err
                } else {
                    var r: SystemActionExecutors.ActionResult? = null
                    SystemActionExecutors.prefetchFile(context, target, reason) { r = it }
                    r ?: SystemActionExecutors.ActionResult(
                        success = true,
                        summary = "prefetch_enqueued",
                        latencyUs = 0,
                        error = null,
                    )
                }
            }
            ACTION_TYPE_KEEP_ALIVE -> {
                val t = target ?: "work:collector_heartbeat"
                SystemActionExecutors.keepAlive(context, t, reason)
            }
            ACTION_TYPE_RELEASE_MEMORY -> {
                SystemActionExecutors.releaseMemory(context, target, reason)
            }
            ACTION_TYPE_NO_OP -> {
                SystemActionExecutors.noOp(context, reason)
            }
            else -> {
                EventRepository.recordInternal(
                    context,
                    "action_dispatch_unsupported",
                    "Unsupported action type",
                    JSONObject()
                        .put("actionType", actionType)
                        .put("target", target ?: JSONObject.NULL)
                        .put("reason", reason),
                )
                SystemActionExecutors.ActionResult(
                    success = false,
                    summary = "unsupported_action",
                    latencyUs = 0,
                    error = "Unsupported action type: $actionType",
                )
            }
        }
    }

    /**
     * Dispatch from a v1-protocol raw JSON payload (backward compat).
     * Extracts action_type + target from the embedded "action" object.
     */
    fun dispatchAuthorizedActionJson(
        context: Context,
        payload: JSONObject,
        reason: String = "authorized_action_json",
    ): Boolean {
        val parsed = parseAuthorizedAction(payload)
        if (parsed == null) {
            EventRepository.recordInternal(
                context,
                "action_dispatch_rejected",
                "AuthorizedAction JSON missing action object or action_type",
                JSONObject()
                    .put("reason", reason)
                    .put("payloadBytes", payload.toString().toByteArray(Charsets.UTF_8).size),
            )
            return false
        }

        val result = dispatch(context, parsed.actionType, parsed.target, reason)
        return result.success
    }

    internal fun parseAuthorizedAction(payload: JSONObject): ParsedAction? {
        val action = payload.optJSONObject("action") ?: return null
        val actionType = action.optString("action_type").takeIf { it.isNotBlank() }
            ?: return null
        val target = action.takeIf { it.has("target") && !it.isNull("target") }?.optString("target")
        return ParsedAction(actionType, target)
    }

    internal data class ParsedAction(
        val actionType: String,
        val target: String?,
    )
}
