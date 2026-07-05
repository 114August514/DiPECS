package com.dipecs.collector

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Build
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.dipecs.collector.services.CombinedIssueExperimentService
import org.json.JSONObject
import java.util.Locale

class DeviceValidationActivity : Activity() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private lateinit var durationInput: EditText
    private lateinit var intervalInput: EditText
    private lateinit var prefetchTargetInput: EditText
    private lateinit var statusText: TextView
    private lateinit var summaryText: TextView
    private lateinit var reportText: TextView
    private var latestSnapshot: JSONObject? = null
    private val refreshRunnable = object : Runnable {
        override fun run() {
            renderFromServiceSnapshot()
            mainHandler.postDelayed(this, 2_000L)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildPage())
        renderFromServiceSnapshot()
    }

    override fun onResume() {
        super.onResume()
        renderFromServiceSnapshot()
        mainHandler.post(refreshRunnable)
    }

    override fun onPause() {
        mainHandler.removeCallbacks(refreshRunnable)
        super.onPause()
    }

    private fun buildPage(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Colors.background)
        }
        root.addView(buildAppTopBar("Device Experiments"))

        val scroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(MATCH, 0, 1f)
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(14), dp(16), dp(18))
        }

        content.addView(buildOperationPanel())
        content.addView(buildLivePanel())
        content.addView(buildReportPanel())
        content.addView(buildGuidePanel())

        scroll.addView(content)
        root.addView(scroll)
        root.addView(buildBottomNav(AppPage.Validation))
        return root
    }

    private fun buildOperationPanel(): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        content.addView(TextView(this).apply {
            text = "Run #97 PrefetchFile, #98 KeepAlive, and #99 ReleaseMemory together on this phone. Default duration is 120 minutes."
            textSize = 13f
            setTextColor(Colors.textSecondary)
            lineHeight = dp(21)
            setPadding(0, 0, 0, dp(10))
        })

        content.addView(sectionLabel("Duration minutes"))
        durationInput = editText("120")
        content.addView(durationInput)

        content.addView(sectionLabel("Sample interval seconds"))
        intervalInput = editText("60")
        content.addView(intervalInput)

        content.addView(sectionLabel("PrefetchFile target"))
        prefetchTargetInput = editText("url:https://raw.githubusercontent.com/114August514/DiPECS/main/README.md")
        content.addView(prefetchTargetInput)

        content.addView(primaryButton("Start combined 2-hour test") { startCombinedTest() })
        content.addView(dangerButton("Stop and export now") { stopCombinedTest() })
        content.addView(secondaryButton("Copy current Markdown report") { copyReport() })

        return wrapCard("Operation", content)
    }

    private fun buildLivePanel(): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        statusText = TextView(this).apply {
            textSize = 14f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Colors.textPrimary)
            lineHeight = dp(22)
        }
        summaryText = TextView(this).apply {
            textSize = 13f
            setTextColor(Colors.textPrimary)
            lineHeight = dp(22)
            setPadding(0, dp(8), 0, 0)
        }
        content.addView(statusText)
        content.addView(summaryText)
        return wrapCard("Live Results", content)
    }

    private fun buildReportPanel(): View {
        reportText = TextView(this).apply {
            textSize = 11f
            typeface = Typeface.MONOSPACE
            setTextColor(Colors.textPrimary)
            lineHeight = dp(17)
            setTextIsSelectable(true)
        }
        return wrapCard("Copyable Report", reportText)
    }

    private fun buildGuidePanel(): View =
        wrapCard("How To Use", TextView(this).apply {
            text = buildString {
                appendLine("1. Keep the app open on this page, set duration to 120 minutes, then tap Start.")
                appendLine("2. Use the phone normally or leave it connected to power. The page updates after every interval.")
                appendLine("3. Tap Stop and export now when you are done, or wait until the duration ends.")
                appendLine("4. Tap Copy current Markdown report and paste it into the project or issue comment.")
                appendLine()
                appendLine("Note: this phone-side run is convenient evidence. Strict #98/#99 memory-pressure acceptance still needs the adb pressure scripts when you want formal issue closure.")
            }
            textSize = 13f
            setTextColor(Colors.textSecondary)
            lineHeight = dp(22)
        })

    private fun startCombinedTest() {
        val duration = durationInput.text.toString().trim().toIntOrNull() ?: 120
        val interval = intervalInput.text.toString().trim().toIntOrNull() ?: 60
        if (duration <= 0) {
            toast("Duration must be positive")
            return
        }
        if (interval <= 0) {
            toast("Interval must be positive")
            return
        }
        val intent = Intent(this, CombinedIssueExperimentService::class.java)
            .setAction(CombinedIssueExperimentService.ACTION_START)
            .putExtra(CombinedIssueExperimentService.EXTRA_DURATION_MINUTES, duration)
            .putExtra(CombinedIssueExperimentService.EXTRA_INTERVAL_SECONDS, interval)
            .putExtra(CombinedIssueExperimentService.EXTRA_PREFETCH_TARGET, prefetchTargetInput.text.toString().trim())
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
        toast("Combined test started in foreground service")
        mainHandler.postDelayed({ renderFromServiceSnapshot() }, 500L)
    }

    private fun stopCombinedTest() {
        startService(
            Intent(this, CombinedIssueExperimentService::class.java)
                .setAction(CombinedIssueExperimentService.ACTION_STOP),
        )
        toast("Stopping after current sample; export will be written")
        mainHandler.postDelayed({ renderFromServiceSnapshot() }, 500L)
    }

    private fun copyReport() {
        val report = latestSnapshot?.optString("markdown_report").orEmpty()
        if (report.isBlank()) {
            toast("No report yet")
            return
        }
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("DiPECS experiment report", report))
        toast("Report copied")
    }

    private fun renderFromServiceSnapshot() {
        val raw = getSharedPreferences(CombinedIssueExperimentService.PREFS_NAME, MODE_PRIVATE)
            .getString(CombinedIssueExperimentService.KEY_SNAPSHOT_JSON, "")
            .orEmpty()
        val snapshot = runCatching { JSONObject(raw) }.getOrNull()
        if (snapshot == null) {
            latestSnapshot = null
            statusText.text = "Status: idle\nProgress: 0.0%\nMessage: Ready\nJSONL: -\nMarkdown: -"
            summaryText.text = "Samples: 0\nStart the foreground service to keep a two-hour run alive across page switches."
            reportText.text = "# DiPECS #97/#98/#99 Combined Device Experiment\n\nNo samples yet."
            return
        }
        render(snapshot)
    }

    private fun render(snapshot: JSONObject) {
        latestSnapshot = snapshot
        val elapsedMs = snapshot.optLong("elapsed_ms")
        val targetDurationMs = snapshot.optLong("target_duration_ms")
        val percent = if (targetDurationMs > 0L) {
            (elapsedMs * 100.0 / targetDurationMs).coerceIn(0.0, 100.0)
        } else {
            0.0
        }
        statusText.text = buildString {
            appendLine("Status: ${if (snapshot.optBoolean("running")) "running in foreground service" else "idle/exported"}")
            appendLine("Progress: ${formatDouble(percent)}% (${formatMillis(elapsedMs)} / ${formatMillis(targetDurationMs)})")
            appendLine("Message: ${snapshot.optString("message").ifBlank { "-" }}")
            appendLine("JSONL: ${snapshot.optString("json_path").ifBlank { "-" }}")
            append("Markdown: ${snapshot.optString("markdown_path").ifBlank { "-" }}")
        }

        val s = snapshot.optJSONObject("summary") ?: JSONObject()
        summaryText.text = buildString {
            appendLine("Samples: ${s.optInt("samples")}")
            appendLine("Prefetch success: ${s.optString("prefetch_success_rate_text", "-")}; mean latency ${s.optLong("prefetch_mean_latency_ms")} ms")
            appendLine("KeepAlive success: ${s.optString("keep_alive_success_rate_text", "-")}; mean latency ${s.optLong("keep_alive_mean_latency_ms")} ms")
            appendLine("ReleaseMemory success: ${s.optString("release_success_rate_text", "-")}")
            appendLine("ReleaseMemory mean available-mem delta: ${s.optLong("release_mean_available_delta_kb")} KB")
            appendLine("Mean PSS delta: ${s.optLong("mean_pss_delta_kb")} KB")
            append("Mean Java heap delta: ${s.optLong("mean_heap_delta_kb")} KB")
        }
        reportText.text = snapshot.optString("markdown_report").ifBlank {
            "# DiPECS #97/#98/#99 Combined Device Experiment\n\nNo report yet."
        }
    }

    private fun editText(defaultText: String): EditText =
        EditText(this).apply {
            setText(defaultText)
            setSingleLine(true)
            textSize = 14f
            setTextColor(Colors.textPrimary)
            setPadding(dp(12), dp(10), dp(12), dp(10))
            background = GradientDrawable().apply {
                setColor(Colors.surfaceMuted)
                cornerRadius = dp(8).toFloat()
                setStroke(1, Colors.border)
            }
            layoutParams = LinearLayout.LayoutParams(MATCH, ViewGroup.LayoutParams.WRAP_CONTENT).apply {
                setMargins(0, 0, 0, dp(8))
            }
        }

    private fun formatMillis(ms: Long): String {
        if (ms <= 0L) return "0 min"
        val minutes = ms / 60_000.0
        return String.format(Locale.US, "%.1f min", minutes)
    }

    private fun formatDouble(value: Double): String =
        String.format(Locale.US, "%.1f", value)

    companion object {
        private val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
    }
}
