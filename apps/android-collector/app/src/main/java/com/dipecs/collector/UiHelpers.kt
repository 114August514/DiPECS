package com.dipecs.collector

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

object Colors {
    val primary = Color.rgb(37, 99, 235)
    val primaryDark = Color.rgb(30, 64, 175)
    val primaryLight = Color.rgb(219, 234, 254)
    val background = Color.rgb(246, 248, 251)
    val cardBg = Color.WHITE
    val textPrimary = Color.rgb(24, 31, 42)
    val textSecondary = Color.rgb(91, 103, 120)
    val success = Color.rgb(16, 185, 129)
    val warning = Color.rgb(217, 119, 6)
    val error = Color.rgb(220, 38, 38)
    val border = Color.rgb(224, 230, 238)
    val tabInactive = Color.rgb(111, 124, 145)
    val surfaceMuted = Color.rgb(241, 245, 249)
}

internal enum class AppPage(val label: String) {
    Home("首页"),
    Dashboard("状态"),
    Sources("采集"),
    Console("控制"),
    Validation("验证"),
}

internal fun Activity.buildAppTopBar(title: String): View =
    LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(18), dp(14), dp(18), dp(14))
        setBackgroundColor(Colors.cardBg)
        elevation = 4f

        addView(TextView(this@buildAppTopBar).apply {
            text = "D"
            textSize = 19f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Color.WHITE)
            gravity = Gravity.CENTER
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(Colors.primary)
            }
            layoutParams = LinearLayout.LayoutParams(dp(42), dp(42)).apply {
                setMargins(0, 0, dp(12), 0)
            }
        })

        addView(TextView(this@buildAppTopBar).apply {
            text = title
            textSize = 20f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Colors.textPrimary)
            layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
        })
    }

@Deprecated("Use buildAppTopBar", ReplaceWith("buildAppTopBar(title)"))
internal fun Activity.buildToolbar(title: String): View = buildAppTopBar(title)

internal fun Activity.buildBottomNav(current: AppPage): View =
    LinearLayout(this).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER
        setPadding(dp(8), dp(8), dp(8), dp(10))
        setBackgroundColor(Colors.cardBg)
        elevation = 8f

        AppPage.values().forEach { page ->
            val selected = page == current
            addView(TextView(this@buildBottomNav).apply {
                text = page.label
                textSize = 13f
                typeface = Typeface.DEFAULT_BOLD
                gravity = Gravity.CENTER
                setTextColor(if (selected) Colors.primaryDark else Colors.tabInactive)
                background = GradientDrawable().apply {
                    cornerRadius = dp(8).toFloat()
                    setColor(if (selected) Colors.primaryLight else Color.TRANSPARENT)
                }
                minHeight = dp(44)
                setOnClickListener {
                    if (!selected) openPage(page)
                }
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).apply {
                    setMargins(dp(2), 0, dp(2), 0)
                }
            })
        }
    }

private fun Activity.openPage(page: AppPage) {
    val target = when (page) {
        AppPage.Home -> MainActivity::class.java
        AppPage.Dashboard -> DashboardActivity::class.java
        AppPage.Sources -> SourcesActivity::class.java
        AppPage.Console -> ConsoleActivity::class.java
        AppPage.Validation -> DeviceValidationActivity::class.java
    }
    if (this::class.java == target) return

    val flags = if (page == AppPage.Home) {
        Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
    } else {
        Intent.FLAG_ACTIVITY_SINGLE_TOP
    }
    startActivity(Intent(this, target).addFlags(flags))
    overridePendingTransition(0, 0)

    if (this !is MainActivity && page != AppPage.Home) {
        finish()
        overridePendingTransition(0, 0)
    }
}

internal fun Context.card(title: String, content: View): View =
    LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(20), dp(18), dp(20), dp(18))
        background = GradientDrawable().apply {
            setColor(Colors.cardBg)
            cornerRadius = dp(8).toFloat()
            setStroke(1, Colors.border)
        }
        elevation = 1.5f
        layoutParams = LinearLayout.LayoutParams(MATCH, WRAP).apply {
            setMargins(0, 0, 0, dp(12))
        }

        addView(TextView(this@card).apply {
            this.text = title
            textSize = 16f
            typeface = Typeface.DEFAULT_BOLD
            setTextColor(Colors.textPrimary)
            setPadding(0, 0, 0, dp(10))
        })
        addView(content)
    }

internal fun Context.wrapCard(title: String, content: View): View = card(title, content)

internal fun Context.sectionLabel(text: String): TextView =
    TextView(this).apply {
        this.text = text
        textSize = 13f
        typeface = Typeface.DEFAULT_BOLD
        setTextColor(Colors.textSecondary)
        setPadding(0, dp(14), 0, dp(6))
        letterSpacing = 0f
    }

internal fun Context.primaryButton(text: String, onClick: () -> Unit): Button =
    Button(this).apply {
        this.text = text
        setAllCaps(false)
        textSize = 15f
        setTextColor(Color.WHITE)
        typeface = Typeface.DEFAULT_BOLD
        background = GradientDrawable().apply {
            setColor(Colors.primary)
            cornerRadius = dp(8).toFloat()
        }
        minHeight = dp(44)
        minimumHeight = dp(44)
        setPadding(dp(18), 0, dp(18), 0)
        setOnClickListener { onClick() }
        layoutParams = LinearLayout.LayoutParams(MATCH, WRAP).apply { setMargins(0, dp(4), 0, dp(4)) }
    }

internal fun Context.secondaryButton(text: String, onClick: () -> Unit): Button =
    Button(this).apply {
        this.text = text
        setAllCaps(false)
        textSize = 15f
        setTextColor(Colors.primary)
        background = GradientDrawable().apply {
            setColor(Color.TRANSPARENT)
            cornerRadius = dp(8).toFloat()
            setStroke(1, Colors.primary)
        }
        minHeight = dp(44)
        minimumHeight = dp(44)
        setPadding(dp(18), 0, dp(18), 0)
        setOnClickListener { onClick() }
        layoutParams = LinearLayout.LayoutParams(MATCH, WRAP).apply { setMargins(0, dp(4), 0, dp(4)) }
    }

internal fun Context.dangerButton(text: String, onClick: () -> Unit): Button =
    Button(this).apply {
        this.text = text
        setAllCaps(false)
        textSize = 14f
        setTextColor(Colors.error)
        background = GradientDrawable().apply {
            setColor(Color.TRANSPARENT)
            cornerRadius = dp(8).toFloat()
            setStroke(1, Colors.error)
        }
        minHeight = dp(40)
        minimumHeight = dp(40)
        setPadding(dp(16), 0, dp(16), 0)
        setOnClickListener { onClick() }
        layoutParams = LinearLayout.LayoutParams(MATCH, WRAP).apply { setMargins(0, dp(2), 0, dp(2)) }
    }

internal fun Context.statusDot(active: Boolean): View =
    View(this).apply {
        layoutParams = LinearLayout.LayoutParams(dp(8), dp(8)).apply {
            setMargins(0, 0, dp(8), 0)
            gravity = Gravity.CENTER_VERTICAL
        }
        background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(if (active) Colors.success else Colors.error)
        }
    }

internal fun Context.toast(message: String) {
    Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
}

@Deprecated("使用 primaryButton / secondaryButton / dangerButton", ReplaceWith("secondaryButton(text, onClick)"))
internal fun Context.rowButton(text: String, onClick: () -> Unit): Button = secondaryButton(text, onClick)

internal fun Context.dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

private val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
private val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT
