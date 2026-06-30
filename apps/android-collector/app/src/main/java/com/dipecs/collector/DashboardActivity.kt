package com.dipecs.collector

import android.app.Activity
import android.graphics.Typeface
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.dipecs.collector.services.CollectorForegroundService
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventStore
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class DashboardActivity : Activity() {

    private lateinit var collectorStatusText: TextView
    private lateinit var socketStatusText: TextView
    private lateinit var permissionStatusText: TextView
    private lateinit var traceSummaryText: TextView
    private lateinit var eventPreviewText: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildPage())
    }

    override fun onResume() {
        super.onResume()
        refreshAll()
    }

    private fun buildPage(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Colors.background)
        }

        root.addView(buildAppTopBar("运行状态"))

        val scroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(MATCH, 0, 1f)
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(14), dp(16), dp(18))
        }

        collectorStatusText = TextView(this).apply {
            textSize = 14f; setTextColor(Colors.textPrimary); setPadding(0, 0, 0, dp(10)); lineHeight = dp(23)
        }
        socketStatusText = TextView(this).apply {
            textSize = 14f; setTextColor(Colors.textPrimary); setPadding(0, 0, 0, dp(12)); lineHeight = dp(23)
        }
        content.addView(wrapCard("运行状态", LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(collectorStatusText)
            addView(socketStatusText)
            addView(primaryButton("启动采集") {
                this@DashboardActivity.startCollectorService(CollectorForegroundService.ACTION_START)
                toast("采集已启动")
            })
            addView(dangerButton("停止采集") {
                this@DashboardActivity.startCollectorService(CollectorForegroundService.ACTION_STOP)
                toast("采集已停止")
            })
        }))

        permissionStatusText = TextView(this).apply {
            textSize = 13f; setTextColor(Colors.textPrimary); lineHeight = dp(23)
        }
        content.addView(wrapCard("权限概览", permissionStatusText))

        traceSummaryText = TextView(this).apply {
            textSize = 13f; setTextColor(Colors.textPrimary); lineHeight = dp(22)
        }
        eventPreviewText = TextView(this).apply {
            textSize = 11f; typeface = Typeface.MONOSPACE; setTextColor(Colors.textSecondary)
            setPadding(0, dp(8), 0, 0); lineHeight = dp(20)
        }
        content.addView(wrapCard("追踪概览", LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(traceSummaryText)
            addView(eventPreviewText)
        }))

        content.addView(wrapCard("隐私边界", TextView(this).apply {
            text = "本地追踪行在存储和导出前已做脱敏处理。\n通知内容、无障碍文本、路径和动作目标均已脱敏。\n仅 rawEvent 非空的行是生产级候选数据。"
            textSize = 12f; setTextColor(Colors.textSecondary); lineHeight = dp(22)
        }))

        scroll.addView(content)
        root.addView(scroll)
        root.addView(buildBottomNav(AppPage.Dashboard))
        return root
    }

    private fun refreshAll() {
        val ctx = this

        val isRunning = CollectorPreferences.isCollectorRunning(ctx)
        collectorStatusText.text = buildString {
            append("采集服务: "); appendLine(if (isRunning) "● 运行中" else "○ 已停止")
            append("最近启动: "); appendLine(formatTs(CollectorPreferences.collectorLastStartedMs(ctx)))
            append("最近停止: "); appendLine(formatTs(CollectorPreferences.collectorLastStoppedMs(ctx)))
            append("设备心跳: "); append(formatTs(CollectorPreferences.lastHeartbeatMs(ctx)))
        }

        val skRunning = CollectorPreferences.isActionSocketListening(ctx)
        socketStatusText.text = buildString {
            append("动作桥接: "); appendLine(if (skRunning) "● 监听中" else "○ 未启动")
            append("状态: "); append(CollectorPreferences.actionSocketStatus(ctx))
        }

        permissionStatusText.text = buildString {
            appendLine("使用情况访问: ${mark(PermissionStatus.hasUsageAccess(ctx))}")
            appendLine("通知监听: ${mark(PermissionStatus.hasNotificationAccess(ctx))}")
            appendLine("无障碍服务: ${mark(PermissionStatus.hasAccessibilityAccess(ctx))}")
            append("通知权限: ${mark(PermissionStatus.hasPostNotifications(ctx))}")
        }

        val store = EventStore(ctx); val stats = store.stats()
        traceSummaryText.text = buildString {
            appendLine("事件总数: ${stats.totalRows}    生产级: ${stats.rawEventRows}")
            appendLine("文件大小: ${formatBytes(stats.fileSizeBytes)}")
            appendLine("最近事件: ${formatTs(stats.latestTimestampMs)}")
            append("数据状态: ${schemaStatus(stats)}")
        }
        eventPreviewText.text = formatRecentEvents(store)
    }

    private fun formatRecentEvents(store: EventStore): String {
        val events = store.readRecent(8).asReversed()
        if (events.isEmpty()) return "暂无追踪事件。启动采集后切换应用或接收通知即可产生事件。"
        val fmt = SimpleDateFormat("HH:mm:ss", Locale.getDefault())
        return events.joinToString("\n\n") { ev ->
            val time = fmt.format(Date(ev.optLong("timestampMs", 0L)))
            val src = ev.optString("source", "?")
            val type = ev.optString("eventType", "?")
            val pkg = ev.optString("packageName", null)?.takeIf { it != "null" && it.isNotBlank() } ?: "-"
            val raw = run {
                val r = ev.optJSONObject("rawEvent") ?: return@run "-"
                val keys = r.keys(); if (keys.hasNext()) keys.next() else "-"
            }
            "[$time] $src / $type\n  应用=$pkg  raw=$raw"
        }
    }

    private fun mark(v: Boolean) = if (v) "● 已授权" else "○ 未授权"
    private fun schemaStatus(s: com.dipecs.collector.storage.TraceStats) = when {
        s.parseErrors > 0 -> "检查 JSON 解析错误"
        s.totalRows == 0 -> "等待设备事件…"
        s.rawEventRows == 0 -> "仅筛查数据，暂无生产级行"
        s.rawEventNullRows > 0 -> "混合数据（生产 + 筛查）"
        else -> "可回放的生产级数据"
    }
    private fun formatTs(ms: Long?) = if (ms == null || ms <= 0L) "(无)" else
        SimpleDateFormat("MM-dd HH:mm:ss", Locale.getDefault()).format(Date(ms))
    private fun formatBytes(bytes: Long) = when {
        bytes >= 1024L * 1024L -> String.format(Locale.getDefault(), "%.1f MiB", bytes / 1024.0 / 1024.0)
        bytes >= 1024L -> String.format(Locale.getDefault(), "%.1f KiB", bytes / 1024.0)
        else -> "$bytes B"
    }

    companion object {
        private val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
    }
}
