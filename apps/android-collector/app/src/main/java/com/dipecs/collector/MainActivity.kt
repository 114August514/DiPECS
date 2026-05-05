package com.dipecs.collector

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.view.View
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import android.widget.Toast
import com.dipecs.collector.net.CloudUploader
import com.dipecs.collector.services.CollectorForegroundService
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import com.dipecs.collector.storage.EventStore
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

class MainActivity : Activity() {
    private lateinit var permissionStatusView: TextView
    private lateinit var traceStatusView: TextView
    private lateinit var eventPreviewView: TextView
    private lateinit var endpointInput: EditText
    private lateinit var apiKeyInput: EditText
    private lateinit var modeSpinner: Spinner
    private lateinit var usageCheck: CheckBox
    private lateinit var notificationCheck: CheckBox
    private lateinit var accessibilityCheck: CheckBox
    private lateinit var deviceContextCheck: CheckBox

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildContentView())
        loadPreferences()
        refreshStatus()
    }

    override fun onResume() {
        super.onResume()
        refreshStatus()
    }

    private fun buildContentView(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(32, 32, 32, 32)
            setBackgroundColor(Color.rgb(248, 250, 252))
        }

        root.addView(TextView(this).apply {
            text = "DiPECS Interface Screening"
            textSize = 24f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.rgb(17, 24, 39))
        })
        root.addView(TextView(this).apply {
            text = "Phase 1 collector dashboard: enable sources, grant interfaces, inspect trace samples."
            textSize = 14f
            setTextColor(Color.rgb(75, 85, 99))
            setPadding(0, 6, 0, 18)
        })

        permissionStatusView = TextView(this).apply {
            textSize = 14f
            setTextColor(Color.rgb(31, 41, 55))
        }
        root.addView(card("Interface status", permissionStatusView))

        usageCheck = sourceCheckBox("Enable UsageStatsManager", CollectorPreferences.isUsageEnabled(this))
        notificationCheck = sourceCheckBox("Enable NotificationListener", CollectorPreferences.isNotificationEnabled(this))
        accessibilityCheck = sourceCheckBox("Enable AccessibilityService", CollectorPreferences.isAccessibilityEnabled(this))
        deviceContextCheck = sourceCheckBox("Enable DeviceContext heartbeat", CollectorPreferences.isDeviceContextEnabled(this))

        root.addView(sourceCard(
            title = "UsageStatsManager",
            detail = "App foreground/background, activity resume/pause, screen/keyguard state.",
            checkBox = usageCheck,
            settingsText = "Grant Usage Access",
            settingsIntent = Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS),
        ))
        root.addView(sourceCard(
            title = "NotificationListenerService",
            detail = "Notification posted/removed, package, category, title/text extras, grouping metadata.",
            checkBox = notificationCheck,
            settingsText = "Grant Notification Access",
            settingsIntent = Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS),
        ))
        root.addView(sourceCard(
            title = "AccessibilityService",
            detail = "Window changes, clicks, focus, text changes, view id, source class and content description.",
            checkBox = accessibilityCheck,
            settingsText = "Grant Accessibility Access",
            settingsIntent = Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS),
        ))
        root.addView(sourceCard(
            title = "DeviceContext",
            detail = "Battery, charging, network, screen state, ringer mode, DND filter.",
            checkBox = deviceContextCheck,
            settingsText = "Grant Notification Runtime Permission",
            settingsIntent = null,
        ) {
            requestNotificationPermission()
        })

        root.addView(uploadConfigCard())
        root.addView(controlCard())

        traceStatusView = TextView(this).apply {
            textSize = 13f
            setTextColor(Color.rgb(31, 41, 55))
        }
        eventPreviewView = TextView(this).apply {
            textSize = 12f
            typeface = Typeface.MONOSPACE
            setTextColor(Color.rgb(17, 24, 39))
        }
        root.addView(card("Trace preview", LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(traceStatusView)
            addView(eventPreviewView)
        }))

        wireSourceToggles()

        return ScrollView(this).apply {
            addView(root)
        }
    }

    private fun uploadConfigCard(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        modeSpinner = Spinner(this)
        modeSpinner.adapter = ArrayAdapter(
            this,
            android.R.layout.simple_spinner_dropdown_item,
            listOf(CollectorPreferences.MODE_MOCK, CollectorPreferences.MODE_LLM),
        )
        modeSpinner.onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
            override fun onItemSelected(parent: AdapterView<*>?, view: View?, position: Int, id: Long) {
                CollectorPreferences.setUploadMode(
                    this@MainActivity,
                    parent?.getItemAtPosition(position)?.toString() ?: CollectorPreferences.MODE_MOCK,
                )
            }

            override fun onNothingSelected(parent: AdapterView<*>?) = Unit
        }
        content.addView(sectionLabel("Upload mode"))
        content.addView(modeSpinner)

        endpointInput = EditText(this).apply {
            hint = "https://example.test/collector"
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_URI
            setSingleLine(true)
        }
        content.addView(sectionLabel("Endpoint"))
        content.addView(endpointInput)

        apiKeyInput = EditText(this).apply {
            hint = "Only used in llm mode"
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
            setSingleLine(true)
        }
        content.addView(sectionLabel("LLM API key"))
        content.addView(apiKeyInput)

        content.addView(rowButton("Save Upload Config") {
            savePreferences()
        })
        return card("Cloud bridge", content)
    }

    private fun controlCard(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        content.addView(rowButton("Start Collector") {
            savePreferences(showToast = false)
            startCollectorService(CollectorForegroundService.ACTION_START)
            toast("Collector started")
        })
        content.addView(rowButton("Stop Collector") {
            startCollectorService(CollectorForegroundService.ACTION_STOP)
            toast("Collector stopped")
        })
        content.addView(rowButton("Upload Recent Events Now") {
            savePreferences(showToast = false)
            CloudUploader.uploadRecent(this, reason = "manual")
            toast("Upload queued")
        })
        content.addView(rowButton("Export JSONL Trace") {
            val target = EventStore(this).exportToExternalFiles()
            toast("Exported to ${target.absolutePath}")
            refreshStatus()
        })
        content.addView(rowButton("Clear Trace") {
            EventStore(this).clear()
            toast("Trace cleared")
            refreshStatus()
        })
        content.addView(rowButton("Refresh Preview") {
            refreshStatus()
        })
        return card("Run controls", content)
    }

    private fun sourceCard(
        title: String,
        detail: String,
        checkBox: CheckBox,
        settingsText: String,
        settingsIntent: Intent?,
        fallbackAction: (() -> Unit)? = null,
    ): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(checkBox)
            addView(TextView(this@MainActivity).apply {
                text = detail
                textSize = 13f
                setTextColor(Color.rgb(75, 85, 99))
                setPadding(0, 0, 0, 8)
            })
            addView(rowButton(settingsText) {
                if (settingsIntent != null) {
                    startActivity(settingsIntent)
                } else {
                    fallbackAction?.invoke()
                }
            })
        }
        return card(title, content)
    }

    private fun sourceCheckBox(text: String, checked: Boolean): CheckBox =
        CheckBox(this).apply {
            this.text = text
            isChecked = checked
            textSize = 15f
            setTextColor(Color.rgb(17, 24, 39))
        }

    private fun wireSourceToggles() {
        usageCheck.setOnCheckedChangeListener { _, enabled ->
            CollectorPreferences.setUsageEnabled(this, enabled)
            recordSourceToggle("usage_stats", enabled)
        }
        notificationCheck.setOnCheckedChangeListener { _, enabled ->
            CollectorPreferences.setNotificationEnabled(this, enabled)
            recordSourceToggle("notification_listener", enabled)
        }
        accessibilityCheck.setOnCheckedChangeListener { _, enabled ->
            CollectorPreferences.setAccessibilityEnabled(this, enabled)
            recordSourceToggle("accessibility", enabled)
        }
        deviceContextCheck.setOnCheckedChangeListener { _, enabled ->
            CollectorPreferences.setDeviceContextEnabled(this, enabled)
            recordSourceToggle("device_context", enabled)
        }
    }

    private fun rowButton(text: String, onClick: () -> Unit): Button =
        Button(this).apply {
            this.text = text
            setAllCaps(false)
            setOnClickListener { onClick() }
        }

    private fun sectionLabel(text: String): TextView =
        TextView(this).apply {
            this.text = text
            textSize = 13f
            setTextColor(Color.rgb(75, 85, 99))
            setPadding(0, 14, 0, 4)
        }

    private fun card(title: String, content: View): View =
        LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 22, 24, 22)
            background = GradientDrawable().apply {
                setColor(Color.WHITE)
                cornerRadius = 16f
                setStroke(1, Color.rgb(226, 232, 240))
            }
            val params = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT,
            )
            params.setMargins(0, 0, 0, 18)
            layoutParams = params

            addView(TextView(this@MainActivity).apply {
                text = title
                textSize = 17f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(Color.rgb(17, 24, 39))
                setPadding(0, 0, 0, 10)
            })
            addView(content)
        }

    private fun loadPreferences() {
        endpointInput.setText(CollectorPreferences.endpoint(this))
        apiKeyInput.setText(CollectorPreferences.apiKey(this))
        val mode = CollectorPreferences.uploadMode(this)
        modeSpinner.setSelection(if (mode == CollectorPreferences.MODE_LLM) 1 else 0)
        usageCheck.isChecked = CollectorPreferences.isUsageEnabled(this)
        notificationCheck.isChecked = CollectorPreferences.isNotificationEnabled(this)
        accessibilityCheck.isChecked = CollectorPreferences.isAccessibilityEnabled(this)
        deviceContextCheck.isChecked = CollectorPreferences.isDeviceContextEnabled(this)
    }

    private fun savePreferences(showToast: Boolean = true) {
        CollectorPreferences.setEndpoint(this, endpointInput.text.toString())
        CollectorPreferences.setApiKey(this, apiKeyInput.text.toString())
        EventRepository.recordInternal(
            this,
            "upload_config_saved",
            "Upload config saved",
            JSONObject().put("mode", CollectorPreferences.uploadMode(this)),
        )
        if (showToast) {
            toast("Saved")
        }
        refreshStatus()
    }

    private fun refreshStatus() {
        permissionStatusView.text = buildString {
            appendLine("Usage access: ${mark(PermissionStatus.hasUsageAccess(this@MainActivity))}")
            appendLine("Notification listener: ${mark(PermissionStatus.hasNotificationAccess(this@MainActivity))}")
            appendLine("Accessibility service: ${mark(PermissionStatus.hasAccessibilityAccess(this@MainActivity))}")
            appendLine("Post notifications: ${mark(PermissionStatus.hasPostNotifications(this@MainActivity))}")
            appendLine()
            appendLine("Enabled sources:")
            appendLine("  UsageStatsManager: ${toggleMark(CollectorPreferences.isUsageEnabled(this@MainActivity))}")
            appendLine("  NotificationListener: ${toggleMark(CollectorPreferences.isNotificationEnabled(this@MainActivity))}")
            appendLine("  AccessibilityService: ${toggleMark(CollectorPreferences.isAccessibilityEnabled(this@MainActivity))}")
            appendLine("  DeviceContext: ${toggleMark(CollectorPreferences.isDeviceContextEnabled(this@MainActivity))}")
        }

        val store = EventStore(this)
        traceStatusView.text = buildString {
            appendLine("Trace file: ${store.traceFile.absolutePath}")
            appendLine("Trace events: ${store.lineCount()}")
            appendLine("Upload endpoint: ${CollectorPreferences.endpoint(this@MainActivity).ifBlank { "(not set)" }}")
            appendLine("Upload mode: ${CollectorPreferences.uploadMode(this@MainActivity)}")
            appendLine()
        }
        eventPreviewView.text = formatRecentEvents(store)
    }

    private fun formatRecentEvents(store: EventStore): String {
        val events = store.readRecent(12).asReversed()
        if (events.isEmpty()) {
            return "No trace events yet. Start the collector, switch apps, post a notification, or interact with UI."
        }
        val formatter = SimpleDateFormat("HH:mm:ss", Locale.US)
        return events.joinToString(separator = "\n\n") { event ->
            val time = formatter.format(Date(event.optLong("timestampMs", 0L)))
            val source = event.optString("source", "?")
            val eventType = event.optString("eventType", "?")
            val pkg = cleanOpt(event, "packageName") ?: "-"
            val text = cleanOpt(event, "text")?.take(80)
            val rawKind = rawEventKind(event) ?: "-"
            buildString {
                append("[$time] $source / $eventType")
                append("\napp=$pkg")
                append("\nraw=$rawKind")
                if (!text.isNullOrBlank()) {
                    append("\ntext=$text")
                }
            }
        }
    }

    private fun cleanOpt(event: JSONObject, key: String): String? {
        if (!event.has(key) || event.isNull(key)) {
            return null
        }
        return event.optString(key).takeIf { it.isNotBlank() && it != "null" }
    }

    private fun rawEventKind(event: JSONObject): String? {
        val rawEvent = event.optJSONObject("rawEvent") ?: return null
        val keys = rawEvent.keys()
        return if (keys.hasNext()) keys.next() else null
    }

    private fun recordSourceToggle(source: String, enabled: Boolean) {
        EventRepository.recordInternal(
            this,
            "source_toggle",
            "$source ${if (enabled) "enabled" else "disabled"}",
            JSONObject()
                .put("source", source)
                .put("enabled", enabled),
        )
        refreshStatus()
    }

    private fun mark(enabled: Boolean): String = if (enabled) "enabled" else "missing"

    private fun toggleMark(enabled: Boolean): String = if (enabled) "enabled" else "disabled"

    private fun startCollectorService(action: String) {
        val intent = Intent(this, CollectorForegroundService::class.java).setAction(action)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && action == CollectorForegroundService.ACTION_START) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
        refreshStatus()
    }

    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_POST_NOTIFICATIONS)
        } else {
            toast("No runtime notification permission needed on this Android version")
        }
    }

    private fun toast(message: String) {
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    companion object {
        private const val REQUEST_POST_NOTIFICATIONS = 3301
    }
}
