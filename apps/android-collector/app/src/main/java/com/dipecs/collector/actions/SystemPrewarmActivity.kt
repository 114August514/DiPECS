package com.dipecs.collector.actions

import android.app.Activity
import android.os.Bundle
import com.dipecs.collector.storage.EventRepository
import org.json.JSONObject

/**
 * Transparent [Activity] that triggers a Zygote process fork for DiPECS-owned
 * components.
 *
 * When `dipecsd` issues a `PreWarmProcess` with `own:*` target, the system
 * bridge launches this Activity in a new task. The Android runtime's Zygote
 * fork + `Application.onCreate()` warm-up happens on the `onCreate` path.
 * The Activity immediately finishes — the warm process stays in the LRU cache
 * for ~30 s, ready for the next interaction.
 *
 * This Activity has `android:theme="@android:style/Theme.NoDisplay"` in the
 * manifest, so the user never sees it.
 *
 * ## System-level requirement
 * When signed with the platform certificate, this Activity bypasses the
 * background-activity-launch restrictions. For a normal app, use
 * [UserVisibleActionNotifier] instead — the user must tap a notification to
 * trigger the launch.
 */
class SystemPrewarmActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val target = intent.getStringExtra(EXTRA_TARGET) ?: "own:resources"
        val reason = intent.getStringExtra(EXTRA_REASON) ?: "prewarm"

        EventRepository.recordInternal(
            this,
            "prewarm_activity_launched",
            "Prewarm Activity started — Zygote fork+Application.onCreate complete",
            JSONObject()
                .put("target", target)
                .put("reason", reason),
        )
        // The Zygote fork has already occurred. Finish immediately.
        finish()
        // Call onDestroy without animation.
        overridePendingTransition(0, 0)
    }

    companion object {
        const val EXTRA_TARGET = "target"
        const val EXTRA_REASON = "reason"
    }
}
