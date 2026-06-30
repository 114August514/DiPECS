package com.dipecs.collector

import android.app.Activity
import android.widget.EditText
import android.widget.LinearLayout
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.services.DebugServiceActions
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

fun Activity.addAuthorizedActionCard(container: LinearLayout) {
    val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

    val input = EditText(this).apply {
        hint = """{"intent_id":"demo","action":{"action_type":"PrefetchFile","target":"url:https://example.test/feed.json","urgency":"IdleTime"},"authorized_at_ms":0}"""
        minLines = 4; maxLines = 8
        setText(CollectorPreferences.authorizedActionJson(this@addAuthorizedActionCard))
    }
    content.addView(sectionLabel("AuthorizedAction JSON"))
    content.addView(input)
    content.addView(primaryButton("保存 JSON") {
        CollectorPreferences.setAuthorizedActionJson(this, input.text.toString())
        EventRepository.recordInternal(this, "authorized_action_saved", "JSON saved")
        toast("已保存")
    })
    content.addView(primaryButton("立即执行") {
        val payload = input.text.toString().trim()
        CollectorPreferences.setAuthorizedActionJson(this, payload)
        val dispatched = runCatching { JSONObject(payload) }
            .map { json -> ActionExecutorBridge.dispatchAuthorizedActionJson(this, json, reason = "manual_authorized_action") }
            .getOrElse { error ->
                EventRepository.recordInternal(this, "authorized_action_rejected",
                    error.message ?: "Invalid JSON",
                    JSONObject().put("payloadBytes", payload.toByteArray(Charsets.UTF_8).size))
                false
            }
        toast(if (dispatched) "已加入队列" else "已拒绝")
    })
    content.addView(secondaryButton("通过服务执行") {
        val payload = input.text.toString().trim()
        CollectorPreferences.setAuthorizedActionJson(this, payload)
        startCollectorService(action = DebugServiceActions.ACTION_EXECUTE_AUTHORIZED_ACTION, authorizedActionJson = payload)
        toast("服务调度已加入队列")
    })
    container.addView(card("授权动作调试（仅 debug）", content))
}
