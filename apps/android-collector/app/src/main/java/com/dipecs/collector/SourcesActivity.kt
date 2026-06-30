package com.dipecs.collector

import android.app.Activity
import android.content.Intent
import android.content.res.ColorStateList
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import android.widget.CheckBox
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

class SourcesActivity : Activity() {

    private lateinit var usageCheck: CheckBox
    private lateinit var notificationCheck: CheckBox
    private lateinit var accessibilityCheck: CheckBox
    private lateinit var deviceContextCheck: CheckBox

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildPage())
        loadPrefs()
        wireToggles()
    }

    override fun onResume() {
        super.onResume()
        loadPrefs()
    }

    private fun buildPage(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Colors.background)
        }

        root.addView(buildAppTopBar("采集源管理"))

        val scroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(MATCH, 0, 1f)
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(14), dp(16), dp(18))
        }

        usageCheck = makeCheckBox("UsageStatsManager（应用使用情况）")
        notificationCheck = makeCheckBox("NotificationListener（通知监听）")
        accessibilityCheck = makeCheckBox("AccessibilityService（无障碍服务，筛查源）")
        deviceContextCheck = makeCheckBox("DeviceContext（设备状态心跳）")

        content.addView(sourceCard(
            "UsageStatsManager",
            "应用前后台切换、Activity 启动/停止、屏幕和锁屏状态。",
            usageCheck,
            "授权使用情况访问",
            Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS),
        ))
        content.addView(sourceCard(
            "NotificationListenerService",
            "通知发布/移除，包含包名、分类、标题/文本摘要和分组元数据。",
            notificationCheck,
            "授权通知监听",
            Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS),
        ))
        content.addView(sourceCard(
            "AccessibilityService",
            "窗口变化、点击、焦点、文本变化等。默认关闭，为筛查源，不用于生产 Rust 管线。",
            accessibilityCheck,
            "授权无障碍访问",
            Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS),
        ))
        content.addView(sourceCard(
            "DeviceContext",
            "电池电量、充电状态、网络类型、屏幕状态、响铃模式。",
            deviceContextCheck,
            "授权通知运行时权限",
            null,
        ) { requestNotificationPermission(this, 3301) })

        scroll.addView(content)
        root.addView(scroll)
        root.addView(buildBottomNav(AppPage.Sources))
        return root
    }

    private fun makeCheckBox(label: String): CheckBox =
        CheckBox(this).apply {
            text = label; textSize = 14f
            setTextColor(Colors.textPrimary)
            buttonTintList = ColorStateList.valueOf(Colors.primary)
        }

    private fun sourceCard(
        title: String, detail: String, checkBox: CheckBox,
        grantLabel: String, settingsIntent: Intent?,
        fallback: (() -> Unit)? = null,
    ): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(checkBox)
        content.addView(TextView(this@SourcesActivity).apply {
            text = detail; textSize = 12f; setTextColor(Colors.textSecondary)
            setPadding(dp(28), dp(6), 0, dp(12)); lineHeight = dp(20)
        })
        content.addView(secondaryButton(grantLabel) {
            if (settingsIntent != null) startActivity(settingsIntent)
            else fallback?.invoke()
        })
        return wrapCard(title, content)
    }

    private fun loadPrefs() {
        usageCheck.isChecked = CollectorPreferences.isUsageEnabled(this)
        notificationCheck.isChecked = CollectorPreferences.isNotificationEnabled(this)
        accessibilityCheck.isChecked = CollectorPreferences.isAccessibilityEnabled(this)
        deviceContextCheck.isChecked = CollectorPreferences.isDeviceContextEnabled(this)
    }

    private fun wireToggles() {
        usageCheck.setOnCheckedChangeListener { _, v ->
            CollectorPreferences.setUsageEnabled(this, v)
            logToggle("usage_stats", v)
        }
        notificationCheck.setOnCheckedChangeListener { _, v ->
            CollectorPreferences.setNotificationEnabled(this, v)
            logToggle("notification_listener", v)
        }
        accessibilityCheck.setOnCheckedChangeListener { _, v ->
            CollectorPreferences.setAccessibilityEnabled(this, v)
            logToggle("accessibility", v)
        }
        deviceContextCheck.setOnCheckedChangeListener { _, v ->
            CollectorPreferences.setDeviceContextEnabled(this, v)
            logToggle("device_context", v)
        }
    }

    private fun logToggle(source: String, enabled: Boolean) {
        EventRepository.recordInternal(this, "source_toggle",
            "$source ${if (enabled) "enabled" else "disabled"}",
            JSONObject().put("source", source).put("enabled", enabled))
    }

    companion object {
        private val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
    }
}
