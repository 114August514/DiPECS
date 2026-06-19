package com.dipecs.collector

import android.widget.EditText
import android.widget.LinearLayout
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.services.DebugServiceActions
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

/**
 * Debug-only extension that adds the manual AuthorizedAction execution panel to
 * [MainActivity]. This code lives in the debug source set so release builds do
 * not expose a UI that can bypass the core lifecycle and forge an action.
 */
fun MainActivity.addAuthorizedActionCard(root: LinearLayout) {
    val content = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
    }

    val authorizedActionInput = EditText(this).apply {
        hint = """{"intent_id":"demo","action":{"action_type":"PrefetchFile","target":"url:https://example.test/feed.json","urgency":"IdleTime"},"authorized_at_ms":0}"""
        minLines = 4
        maxLines = 8
        setText(CollectorPreferences.authorizedActionJson(this@addAuthorizedActionCard))
    }
    content.addView(sectionLabel("AuthorizedAction JSON"))
    content.addView(authorizedActionInput)
    content.addView(rowButton("Save AuthorizedAction JSON") {
        CollectorPreferences.setAuthorizedActionJson(this, authorizedActionInput.text.toString())
        EventRepository.recordInternal(
            this,
            "authorized_action_saved",
            "AuthorizedAction JSON saved",
        )
        toast("AuthorizedAction JSON saved")
        refreshStatus()
    })
    content.addView(rowButton("Run AuthorizedAction Now") {
        val payload = authorizedActionInput.text.toString().trim()
        CollectorPreferences.setAuthorizedActionJson(this, payload)
        val dispatched = runCatching { JSONObject(payload) }
            .map { json ->
                ActionExecutorBridge.dispatchAuthorizedActionJson(
                    this,
                    json,
                    reason = "manual_authorized_action",
                )
            }
            .getOrElse { error ->
                EventRepository.recordInternal(
                    this,
                    "authorized_action_rejected",
                    error.message ?: "Invalid AuthorizedAction JSON",
                    JSONObject().put("payload", payload.take(2048)),
                )
                false
            }
        if (dispatched) {
            toast("AuthorizedAction queued")
        } else {
            toast("AuthorizedAction rejected")
        }
        refreshStatus()
    })
    content.addView(rowButton("Run AuthorizedAction Via Service") {
        val payload = authorizedActionInput.text.toString().trim()
        CollectorPreferences.setAuthorizedActionJson(this, payload)
        startCollectorService(
            action = DebugServiceActions.ACTION_EXECUTE_AUTHORIZED_ACTION,
            authorizedActionJson = payload,
        )
        toast("AuthorizedAction service dispatch queued")
    })
    root.addView(card("Authorized action bridge (debug)", content))
}
