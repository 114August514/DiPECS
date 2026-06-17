package com.dipecs.collector

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
import android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
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
import android.widget.Toast
import com.dipecs.collector.actions.ActionExecutorBridge
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
    private lateinit var authorizedActionInput: EditText
    private lateinit var actionSocketPortInput: EditText
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
        root.addView(authorizedActionCard())
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
            text = "Supports url:http(s) and persisted uri:content:// targets. Prefetched content is stored in app cache."
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

    private fun authorizedActionCard(): View {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }

        authorizedActionInput = EditText(this).apply {
            hint = """{"intent_id":"demo","action":{"action_type":"PrefetchFile","target":"url:https://example.test/feed.json","urgency":"IdleTime"},"authorized_at_ms":0}"""
            minLines = 4
            maxLines = 8
            setText(CollectorPreferences.authorizedActionJson(this@MainActivity))
        }
        content.addView(sectionLabel("AuthorizedAction JSON"))
        content.addView(authorizedActionInput)
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
        content.addView(rowButton("Save AuthorizedAction JSON") {
            CollectorPreferences.setAuthorizedActionJson(this@MainActivity, authorizedActionInput.text.toString())
            if (!saveActionSocketPort()) {
                return@rowButton
            }
            EventRepository.recordInternal(
                this@MainActivity,
                "authorized_action_saved",
                "AuthorizedAction JSON saved",
            )
            toast("AuthorizedAction JSON saved")
            refreshStatus()
        })
        content.addView(rowButton("Run AuthorizedAction Now") {
            val payload = authorizedActionInput.text.toString().trim()
            CollectorPreferences.setAuthorizedActionJson(this@MainActivity, payload)
            val dispatched = runCatching { JSONObject(payload) }
                .map { json ->
                    ActionExecutorBridge.dispatchAuthorizedActionJson(
                        this@MainActivity,
                        json,
                        reason = "manual_authorized_action",
                    )
                }
                .getOrElse { error ->
                    EventRepository.recordInternal(
                        this@MainActivity,
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
            CollectorPreferences.setAuthorizedActionJson(this@MainActivity, payload)
            startCollectorService(
                action = CollectorForegroundService.ACTION_EXECUTE_AUTHORIZED_ACTION,
                authorizedActionJson = payload,
            )
            toast("AuthorizedAction service dispatch queued")
        })
        return card("Authorized action bridge", content)
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
        prefetchTargetInput.setText(CollectorPreferences.prefetchTarget(this))
        authorizedActionInput.setText(CollectorPreferences.authorizedActionJson(this))
        actionSocketPortInput.setText(CollectorPreferences.actionSocketPort(this).toString())
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
        CollectorPreferences.setPrefetchTarget(this, prefetchTargetInput.text.toString())
        CollectorPreferences.setAuthorizedActionJson(this, authorizedActionInput.text.toString())
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
            appendLine("Prefetch target: ${CollectorPreferences.prefetchTarget(this@MainActivity).ifBlank { "(not set)" }}")
            appendLine("AuthorizedAction JSON: ${if (CollectorPreferences.authorizedActionJson(this@MainActivity).isBlank()) "(not set)" else "configured"}")
            appendLine("Action socket: 127.0.0.1:${CollectorPreferences.actionSocketPort(this@MainActivity)}")
            appendLine("Action socket token: ${redactSecret(CollectorPreferences.actionSocketToken(this@MainActivity))}")
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

    private fun redactSecret(secret: String): String =
        if (secret.isBlank()) {
            "(not set)"
        } else {
            "configured (...${secret.takeLast(6)})"
        }

    private fun startCollectorService(
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

    private fun toast(message: String) {
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
    }

    companion object {
        private const val REQUEST_POST_NOTIFICATIONS = 3301
        private const val REQUEST_OPEN_DOCUMENT = 3302
    }
}
