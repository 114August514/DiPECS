package com.dipecs.collector

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
import android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION
import android.graphics.Color
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
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
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.actions.AccessibleContentPrefetcher
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
    private lateinit var prefetchTargetInput: EditText
    private lateinit var actionSocketPortInput: EditText
    private lateinit var modeSpinner: Spinner
    private lateinit var uploadEnabledCheck: CheckBox
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

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_OPEN_DOCUMENT || resultCode != RESULT_OK) {
            return
        }
        val uri = data?.data ?: return
        val grantFlags = data.flags and FLAG_GRANT_READ_URI_PERMISSION
        if (grantFlags != 0) {
            runCatching {
                contentResolver.takePersistableUriPermission(uri, grantFlags)
            }
        }
        CollectorPreferences.setPrefetchTarget(this, "uri:$uri")
        prefetchTargetInput.setText(CollectorPreferences.prefetchTarget(this))
        EventRepository.recordInternal(
            this,
            "prefetch_uri_selected",
            "Prefetch document URI selected",
            JSONObject().put("target", CollectorPreferences.prefetchTarget(this)),
        )
        toast("Saved URI prefetch target")
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
            text = "Android public-API bridge: production rawEvent sources plus optional interface screening."
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
        root.addView(prefetchCard())
        root.addView(actionSocketCard())
        addAuthorizedActionCard(root)
        root.addView(privacyCard())
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
        uploadEnabledCheck = sourceCheckBox(
            "Enable periodic upload",
            CollectorPreferences.isUploadEnabled(this),
        )
        content.addView(uploadEnabledCheck)
        content.addView(TextView(this).apply {
            text = "Manual upload is still available for validation. Periodic upload stays off unless this is enabled."
            textSize = 12f
            setTextColor(Color.rgb(75, 85, 99))
            setPadding(0, 4, 0, 8)
        })

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
            confirmExportTrace()
        })
        content.addView(rowButton("Clear Trace") {
            confirmClearTrace()
        })
        content.addView(rowButton("Refresh Preview") {
            refreshStatus()
        })
        return card("Run controls", content)
    }

    private fun privacyCard(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        content.addView(TextView(this).apply {
            text = buildString {
                appendLine("Local trace rows are sanitized before they are stored or exported.")
                appendLine("Notification title/text, accessibility text, socket payloads, cache paths, and action targets are redacted.")
                appendLine("Only rows with non-null rawEvent are production Rust ingress candidates.")
                append("AccessibilityService is a screening source and stays disabled by default.")
            }
            textSize = 13f
            setTextColor(Color.rgb(75, 85, 99))
        })
        return card("Privacy boundary", content)
    }

    private fun prefetchCard(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        prefetchTargetInput = EditText(this).apply {
            hint = "url:https://example.test/feed.json or uri:content://..."
            inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_URI
            setSingleLine(true)
        }
        content.addView(sectionLabel("Prefetch target"))
        content.addView(prefetchTargetInput)
        content.addView(TextView(this).apply {
            text = "Supports url:https:// and persisted uri:content:// targets. Prefetched content is stored in app cache with a 24h TTL."
            textSize = 12f
            setTextColor(Color.rgb(75, 85, 99))
            setPadding(0, 8, 0, 10)
        })
        content.addView(rowButton("Save Prefetch Target") {
            CollectorPreferences.setPrefetchTarget(this@MainActivity, prefetchTargetInput.text.toString())
            EventRepository.recordInternal(
                this@MainActivity,
                "prefetch_target_saved",
                "Prefetch target saved",
                JSONObject().put("target", CollectorPreferences.prefetchTarget(this@MainActivity)),
            )
            toast("Prefetch target saved")
            refreshStatus()
        })
        content.addView(rowButton("Run Prefetch Now") {
            val target = prefetchTargetInput.text.toString().trim()
            CollectorPreferences.setPrefetchTarget(this@MainActivity, target)
            ActionExecutorBridge.dispatch(
                this@MainActivity,
                ActionExecutorBridge.ACTION_TYPE_PREFETCH_FILE,
                target,
                reason = "manual",
            )
            toast("Prefetch queued")
            refreshStatus()
        })
        content.addView(rowButton("Run Prefetch Via Service") {
            val target = prefetchTargetInput.text.toString().trim()
            CollectorPreferences.setPrefetchTarget(this@MainActivity, target)
            startCollectorService(
                action = CollectorForegroundService.ACTION_PREFETCH_NOW,
                prefetchTarget = target,
            )
            toast("Service prefetch queued")
        })
        content.addView(rowButton("Release Own Prefetch Cache") {
            ActionExecutorBridge.dispatch(
                this@MainActivity,
                ActionExecutorBridge.ACTION_TYPE_RELEASE_MEMORY,
                "cache:prefetch",
                reason = "manual",
            )
            toast("ReleaseMemory queued")
            refreshStatus()
        })
        content.addView(rowButton("Schedule KeepAlive Job") {
            ActionExecutorBridge.dispatch(
                this@MainActivity,
                ActionExecutorBridge.ACTION_TYPE_KEEP_ALIVE,
                "work:collector_heartbeat",
                reason = "manual",
            )
            toast("KeepAlive job scheduled")
            refreshStatus()
        })
        content.addView(rowButton("Warm Own Resources") {
            ActionExecutorBridge.dispatch(
                this@MainActivity,
                ActionExecutorBridge.ACTION_TYPE_PREWARM_PROCESS,
                "own:resources",
                reason = "manual",
            )
            toast("Own resources warmed")
            refreshStatus()
        })
        content.addView(rowButton("Post User-Visible Hint") {
            ActionExecutorBridge.dispatch(
                this@MainActivity,
                ActionExecutorBridge.ACTION_TYPE_PREWARM_PROCESS,
                "notif:review_action",
                reason = "manual",
            )
            toast("Action hint requested")
            refreshStatus()
        })
        content.addView(rowButton("Pick Document URI") {
            val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "*/*"
                addFlags(FLAG_GRANT_READ_URI_PERMISSION or FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
            }
            startActivityForResult(intent, REQUEST_OPEN_DOCUMENT)
        })
        return card("Prefetch action", content)
    }

    private fun actionSocketCard(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        actionSocketPortInput = EditText(this).apply {
            hint = CollectorPreferences.DEFAULT_ACTION_SOCKET_PORT.toString()
            inputType = android.text.InputType.TYPE_CLASS_NUMBER
            setSingleLine(true)
            setText(CollectorPreferences.actionSocketPort(this@MainActivity).toString())
        }
        content.addView(sectionLabel("Action socket port"))
        content.addView(actionSocketPortInput)
        content.addView(TextView(this).apply {
            text = "Socket payloads require auth_token. Provide the token out-of-band with aios-cli --auth-token. From a desktop host, usually run adb forward tcp:PORT tcp:PORT first."
            textSize = 12f
            setTextColor(Color.rgb(75, 85, 99))
            setPadding(0, 8, 0, 10)
        })
        content.addView(rowButton("Copy Action Socket Token") {
            copyActionSocketToken()
        })
        return card("Action socket bridge", content)
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

    private fun loadPreferences() {
        endpointInput.setText(CollectorPreferences.endpoint(this))
        apiKeyInput.setText(CollectorPreferences.apiKey(this))
        prefetchTargetInput.setText(CollectorPreferences.prefetchTarget(this))
        actionSocketPortInput.setText(CollectorPreferences.actionSocketPort(this).toString())
        uploadEnabledCheck.isChecked = CollectorPreferences.isUploadEnabled(this)
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
        CollectorPreferences.setUploadEnabled(this, uploadEnabledCheck.isChecked)
        CollectorPreferences.setPrefetchTarget(this, prefetchTargetInput.text.toString())
        if (!saveActionSocketPort()) {
            return
        }
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

    internal fun refreshStatus() {
        permissionStatusView.text = buildString {
            appendLine("Usage access: ${mark(PermissionStatus.hasUsageAccess(this@MainActivity))}")
            appendLine("Notification listener: ${mark(PermissionStatus.hasNotificationAccess(this@MainActivity))}")
            appendLine("Accessibility service: ${mark(PermissionStatus.hasAccessibilityAccess(this@MainActivity))}")
            appendLine("Post notifications: ${mark(PermissionStatus.hasPostNotifications(this@MainActivity))}")
            appendLine()
            appendLine("Runtime:")
            appendLine("  Collector service: ${runtimeMark(CollectorPreferences.isCollectorRunning(this@MainActivity))}")
            appendLine("  Last start: ${formatTimestamp(CollectorPreferences.collectorLastStartedMs(this@MainActivity))}")
            appendLine("  Last stop: ${formatTimestamp(CollectorPreferences.collectorLastStoppedMs(this@MainActivity))}")
            appendLine("  Last DeviceContext heartbeat: ${formatTimestamp(CollectorPreferences.lastHeartbeatMs(this@MainActivity))}")
            appendLine("  Action socket: ${runtimeMark(CollectorPreferences.isActionSocketListening(this@MainActivity))}")
            appendLine("  Socket status: ${CollectorPreferences.actionSocketStatus(this@MainActivity)}")
            appendLine("  Socket status time: ${formatTimestamp(CollectorPreferences.actionSocketStatusMs(this@MainActivity))}")
            appendLine()
            appendLine("Enabled sources:")
            appendLine("  UsageStatsManager: ${toggleMark(CollectorPreferences.isUsageEnabled(this@MainActivity))}")
            appendLine("  NotificationListener: ${toggleMark(CollectorPreferences.isNotificationEnabled(this@MainActivity))}")
            appendLine("  AccessibilityService: ${toggleMark(CollectorPreferences.isAccessibilityEnabled(this@MainActivity))}")
            appendLine("  DeviceContext: ${toggleMark(CollectorPreferences.isDeviceContextEnabled(this@MainActivity))}")
        }

        val store = EventStore(this)
        val stats = store.stats()
        traceStatusView.text = buildString {
            appendLine("Trace file: ${store.traceFile.absolutePath}")
            appendLine("Trace events: ${stats.totalRows}")
            appendLine("Trace size: ${formatBytes(stats.fileSizeBytes)}")
            appendLine("Latest event: ${formatTimestamp(stats.latestTimestampMs)}")
            appendLine("Production rawEvent rows: ${stats.rawEventRows}/${stats.totalRows}")
            appendLine("Screening/rawEvent-null rows: ${stats.rawEventNullRows}")
            appendLine("Schema status: ${schemaStatus(stats)}")
            appendLine("Latest rawEvent kind: ${stats.latestRawEventKind ?: "(none)"}")
            appendLine("Parse errors: ${stats.parseErrors}")
            appendLine("Latest parse error: ${stats.latestParseError ?: "(none)"}")
            appendLine("Sources: ${formatCounts(stats.sourceCounts)}")
            appendLine("Event types: ${formatCounts(stats.eventTypeCounts)}")
            appendLine("rawEvent kinds: ${formatCounts(stats.rawEventKindCounts)}")
            appendLine("Last export: ${formatLastExport()}")
            appendLine("Upload endpoint: ${redactEndpoint(CollectorPreferences.endpoint(this@MainActivity))}")
            appendLine("Upload mode: ${CollectorPreferences.uploadMode(this@MainActivity)}")
            appendLine("Periodic upload: ${toggleMark(CollectorPreferences.isUploadEnabled(this@MainActivity))}")
            appendLine("Prefetch target: ${redactTarget(CollectorPreferences.prefetchTarget(this@MainActivity))}")
            appendLine("Action socket: 127.0.0.1:${CollectorPreferences.actionSocketPort(this@MainActivity)}")
            appendLine("Action socket token: ${redactSecret(CollectorPreferences.actionSocketToken(this@MainActivity))}")
            appendLine()
            appendLine("Developer commands:")
            append(buildDeveloperCommands())
            appendLine()
        }
        eventPreviewView.text = formatRecentEvents(store)
    }

    private fun confirmExportTrace() {
        val stats = EventStore(this).stats()
        AlertDialog.Builder(this)
            .setTitle("Export sanitized trace?")
            .setMessage(
                "This writes a sanitized JSONL copy to external app files. " +
                    "Rows: ${stats.totalRows}, rawEvent rows: ${stats.rawEventRows}. " +
                    "Review the file before sharing it outside the device.",
            )
            .setPositiveButton("Export") { _, _ ->
                val target = EventStore(this).exportToExternalFiles()
                CollectorPreferences.setLastExport(this, target.absolutePath, System.currentTimeMillis())
                toast("Exported to ${target.absolutePath}")
                refreshStatus()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun confirmClearTrace() {
        AlertDialog.Builder(this)
            .setTitle("Clear local trace?")
            .setMessage("This removes the local JSONL trace and prefetch cache from app-private storage. Export first if you need a sanitized copy.")
            .setPositiveButton("Clear") { _, _ ->
                EventStore(this).clear()
                val deletedCacheFiles = AccessibleContentPrefetcher.clearCache(this)
                toast("Trace cleared; prefetch cache files deleted: $deletedCacheFiles")
                refreshStatus()
            }
            .setNegativeButton("Cancel", null)
            .show()
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

    private fun runtimeMark(enabled: Boolean): String = if (enabled) "running" else "stopped"

    private fun redactSecret(secret: String): String =
        if (secret.isBlank()) {
            "(not set)"
        } else {
            "configured (...${secret.takeLast(6)})"
        }

    private fun redactTarget(target: String): String {
        if (target.isBlank()) {
            return "(not set)"
        }
        return when {
            target.startsWith("url:https://") -> {
                val host = runCatching { java.net.URL(target.removePrefix("url:")).host }.getOrNull()
                "url:https://${host ?: "..."}/..."
            }
            target.startsWith("uri:content://") -> {
                val uri = android.net.Uri.parse(target.removePrefix("uri:"))
                "uri:content://${uri.authority ?: "..."}/..."
            }
            else -> "configured (...${target.takeLast(6)})"
        }
    }

    private fun redactEndpoint(endpoint: String): String {
        if (endpoint.isBlank()) {
            return "(not set)"
        }
        val url = runCatching { java.net.URL(endpoint) }.getOrNull()
            ?: return "configured (...${endpoint.takeLast(6)})"
        val port = if (url.port > 0) ":${url.port}" else ""
        return "${url.protocol}://${url.host}$port/..."
    }

    private fun formatTimestamp(timestampMs: Long?): String {
        if (timestampMs == null || timestampMs <= 0L) {
            return "(none)"
        }
        return SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date(timestampMs))
    }

    private fun formatLastExport(): String {
        val path = CollectorPreferences.lastExportPath(this)
        if (path.isBlank()) {
            return "(none)"
        }
        return "$path at ${formatTimestamp(CollectorPreferences.lastExportMs(this))}"
    }

    private fun schemaStatus(stats: com.dipecs.collector.storage.TraceStats): String =
        when {
            stats.parseErrors > 0 -> "check JSON parse errors before replay"
            stats.totalRows == 0 -> "waiting for device events"
            stats.rawEventRows == 0 -> "screening only; no Rust production rows yet"
            stats.rawEventNullRows > 0 -> "mixed production and screening rows"
            else -> "production replay candidate"
        }

    private fun buildDeveloperCommands(): String {
        val port = CollectorPreferences.actionSocketPort(this)
        val deviceExportPath = "/sdcard/Android/data/$packageName/files/traces/actions.jsonl"
        val localTrace = "data/traces/android_real_device_sample.redacted.jsonl"
        return buildString {
            appendLine("adb pull $deviceExportPath $localTrace")
            appendLine("cargo run -p aios-cli -- replay $localTrace --stages policy --audit data/evaluation/android_real_device.audit.ndjson")
            appendLine("cargo run -p aios-daemon --bin dipecsd -- --no-daemon --android-trace-jsonl $localTrace --trace-output data/evaluation/android_real_device.runtime.ndjson")
            appendLine("adb forward tcp:$port tcp:$port")
            append("cargo run -p aios-cli -- send-authorized-action --auth-token <copied-token> --host 127.0.0.1 --port $port")
        }
    }

    private fun formatBytes(bytes: Long): String =
        when {
            bytes >= 1024L * 1024L -> String.format(Locale.US, "%.1f MiB", bytes / 1024.0 / 1024.0)
            bytes >= 1024L -> String.format(Locale.US, "%.1f KiB", bytes / 1024.0)
            else -> "$bytes B"
        }

    private fun formatCounts(counts: Map<String, Int>): String {
        if (counts.isEmpty()) {
            return "(none)"
        }
        return counts.entries
            .sortedWith(compareByDescending<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
            .take(5)
            .joinToString { "${it.key}=${it.value}" }
    }

    internal fun startCollectorService(
        action: String,
        prefetchTarget: String? = null,
        authorizedActionJson: String? = null,
    ) {
        val intent = Intent(this, CollectorForegroundService::class.java).setAction(action)
        if (!prefetchTarget.isNullOrBlank()) {
            intent.putExtra(CollectorForegroundService.EXTRA_PREFETCH_TARGET, prefetchTarget)
        }
        if (!authorizedActionJson.isNullOrBlank()) {
            intent.putExtra(
                CollectorForegroundService.EXTRA_AUTHORIZED_ACTION_JSON,
                authorizedActionJson,
            )
        }
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

    private fun saveActionSocketPort(): Boolean {
        val text = actionSocketPortInput.text.toString().trim()
        val port = text.toIntOrNull()
        if (port == null || port !in 1024..65535) {
            toast("Action socket port must be between 1024 and 65535")
            actionSocketPortInput.setText(CollectorPreferences.actionSocketPort(this).toString())
            return false
        }
        CollectorPreferences.setActionSocketPort(this, port)
        return true
    }

    private fun copyActionSocketToken() {
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        if (clipboard == null) {
            toast("Clipboard service unavailable")
            return
        }
        val token = CollectorPreferences.actionSocketToken(this)
        val clip = ClipData.newPlainText("DiPECS action socket token", token)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            clip.description.extras = PersistableBundle().apply {
                putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
            }
        }
        clipboard.setPrimaryClip(clip)
        toast("Action socket token copied")
        refreshStatus()
    }

    companion object {
        private const val REQUEST_POST_NOTIFICATIONS = 3301
        private const val REQUEST_OPEN_DOCUMENT = 3302
    }
}
