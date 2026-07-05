package com.dipecs.collector

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.dipecs.collector.validation.CombinedExperimentSnapshot
import com.dipecs.collector.validation.CombinedIssueExperimentRunner
import java.util.Locale

class DeviceValidationActivity : Activity(), CombinedIssueExperimentRunner.Listener {
    private val mainHandler = Handler(Looper.getMainLooper())
    private lateinit var runner: CombinedIssueExperimentRunner
    private lateinit var durationInput: EditText
    private lateinit var intervalInput: EditText
    private lateinit var prefetchTargetInput: EditText
    private lateinit var statusText: TextView
    private lateinit var summaryText: TextView
    private lateinit var reportText: TextView
    private var latestSnapshot: CombinedExperimentSnapshot? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        runner = CombinedIssueExperimentRunner(this, this)
        setContentView(buildPage())
        render(runner.snapshot("Ready"))
    }

    override fun onCombinedSnapshot(snapshot: CombinedExperimentSnapshot) {
        mainHandler.post { render(snapshot) }
    }

    override fun onCombinedFinished(snapshot: CombinedExperimentSnapshot) {
        mainHandler.post {
            render(snapshot)
            toast("Combined test finished and exported")
        }
    }

    override fun onDestroy() {
        if (latestSnapshot?.running == true) {
            runner.stop()
        }
        super.onDestroy()
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
        runner.start(
            durationMinutes = duration,
            intervalSeconds = interval,
            target = prefetchTargetInput.text.toString().trim(),
        )
        toast("Combined test started")
    }

    private fun stopCombinedTest() {
        runner.stop()
        toast("Stopping after current sample; export will be written")
    }

    private fun copyReport() {
        val report = latestSnapshot?.markdownReport.orEmpty()
        if (report.isBlank()) {
            toast("No report yet")
            return
        }
        val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("DiPECS experiment report", report))
        toast("Report copied")
    }

    private fun render(snapshot: CombinedExperimentSnapshot) {
        latestSnapshot = snapshot
        val percent = if (snapshot.targetDurationMs > 0L) {
            (snapshot.elapsedMs * 100.0 / snapshot.targetDurationMs).coerceIn(0.0, 100.0)
        } else {
            0.0
        }
        statusText.text = buildString {
            appendLine("Status: ${if (snapshot.running) "running" else "idle/exported"}")
            appendLine("Progress: ${formatDouble(percent)}% (${formatMillis(snapshot.elapsedMs)} / ${formatMillis(snapshot.targetDurationMs)})")
            appendLine("Message: ${snapshot.message.ifBlank { "-" }}")
            appendLine("JSONL: ${snapshot.jsonPath.ifBlank { "-" }}")
            append("Markdown: ${snapshot.markdownPath.ifBlank { "-" }}")
        }

        val s = snapshot.summary
        summaryText.text = buildString {
            appendLine("Samples: ${s.samples}")
            appendLine("Prefetch success: ${s.prefetchSuccessRateText}; mean latency ${s.prefetchMeanLatencyMs} ms")
            appendLine("KeepAlive success: ${s.keepAliveSuccessRateText}; mean latency ${s.keepAliveMeanLatencyMs} ms")
            appendLine("ReleaseMemory success: ${s.releaseSuccessRateText}")
            appendLine("ReleaseMemory mean available-mem delta: ${s.releaseMeanAvailableDeltaKb} KB")
            appendLine("Mean PSS delta: ${s.meanPssDeltaKb} KB")
            append("Mean Java heap delta: ${s.meanHeapDeltaKb} KB")
        }
        reportText.text = snapshot.markdownReport
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
