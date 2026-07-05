package com.dipecs.collector

import android.app.Activity
import android.content.Intent
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.dipecs.collector.services.CollectorForegroundService
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventStore

class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildHomePage())
        if (BuildConfig.DEBUG && intent?.getBooleanExtra("auto_start", false) == true) {
            startCollectorService(CollectorForegroundService.ACTION_START)
        }
    }

    override fun onResume() {
        super.onResume()
        setContentView(buildHomePage())
    }

    private fun buildHomePage(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Colors.background)
        }
        root.addView(buildAppTopBar("DiPECS"))

        val scroll = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(MATCH, 0, 1f)
            isFillViewport = false
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(14), dp(16), dp(18))
        }

        content.addView(buildHero())
        content.addView(buildStatusStrip())
        content.addView(navCard(
            title = "运行状态",
            description = "查看采集服务、权限、动作桥接和最近事件。",
            glyph = "S",
            color = Colors.success,
        ) { startActivity(Intent(this, DashboardActivity::class.java)) })
        content.addView(navCard(
            title = "采集源管理",
            description = "管理 UsageStats、通知监听、无障碍服务和设备状态。",
            glyph = "C",
            color = Colors.primary,
        ) { startActivity(Intent(this, SourcesActivity::class.java)) })
        content.addView(navCard(
            title = "操作控制台",
            description = "执行预取、动作 socket、数据导出和调试任务。",
            glyph = "A",
            color = Colors.warning,
        ) { startActivity(Intent(this, ConsoleActivity::class.java)) })
        content.addView(navCard(
            title = "真机验证",
            description = "本地-only 跑通项目、#97、#98、#99；不需要 API key。",
            glyph = "V",
            color = Colors.primaryDark,
        ) { startActivity(Intent(this, DeviceValidationActivity::class.java)) })

        scroll.addView(content)
        root.addView(scroll)
        root.addView(buildBottomNav(AppPage.Home))
        return root
    }

    private fun buildHero(): View =
        LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(26), dp(20), dp(24))
            background = GradientDrawable().apply {
                setColor(Colors.cardBg)
                cornerRadius = dp(8).toFloat()
                setStroke(1, Colors.border)
            }
            elevation = 2f
            layoutParams = LinearLayout.LayoutParams(MATCH, WRAP).apply {
                setMargins(0, 0, 0, dp(12))
            }

            addView(TextView(this@MainActivity).apply {
                text = "Android 行为采集与真机验证"
                textSize = 23f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(Colors.textPrimary)
                setPadding(0, 0, 0, dp(10))
            })
            addView(TextView(this@MainActivity).apply {
                text = "默认本地运行。真机验证页会排除云端 LLM，不会要求 API key。"
                textSize = 13f
                setTextColor(Colors.textSecondary)
                lineHeight = dp(21)
                setPadding(0, 0, 0, dp(12))
            })
            addView(LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.HORIZONTAL
                addView(primaryButton("启动采集") {
                    startCollectorService(CollectorForegroundService.ACTION_START)
                    toast("采集已启动")
                }.apply {
                    layoutParams = LinearLayout.LayoutParams(0, WRAP, 1f).apply { setMargins(0, 0, dp(6), 0) }
                })
                addView(secondaryButton("真机验证") {
                    startActivity(Intent(this@MainActivity, DeviceValidationActivity::class.java))
                }.apply {
                    layoutParams = LinearLayout.LayoutParams(0, WRAP, 1f).apply { setMargins(dp(6), 0, 0, 0) }
                })
            })
        }

    private fun buildStatusStrip(): View {
        val stats = EventStore(this).stats()
        val running = CollectorPreferences.isCollectorRunning(this)
        val socket = CollectorPreferences.isActionSocketListening(this)
        return LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(0, 0, 0, dp(2))
            addView(metricPill("采集", if (running) "运行中" else "未启动", if (running) Colors.success else Colors.tabInactive))
            addView(metricPill("事件", stats.totalRows.toString(), Colors.primary))
            addView(metricPill("桥接", if (socket) "监听" else "关闭", if (socket) Colors.success else Colors.tabInactive))
        }
    }

    private fun metricPill(label: String, value: String, color: Int): View =
        LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(dp(10), dp(12), dp(10), dp(12))
            background = GradientDrawable().apply {
                setColor(Colors.cardBg)
                cornerRadius = dp(8).toFloat()
                setStroke(1, Colors.border)
            }
            layoutParams = LinearLayout.LayoutParams(0, WRAP, 1f).apply {
                setMargins(dp(3), 0, dp(3), dp(12))
            }
            addView(TextView(this@MainActivity).apply {
                text = value
                textSize = 18f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(color)
                gravity = Gravity.CENTER
            })
            addView(TextView(this@MainActivity).apply {
                text = label
                textSize = 12f
                setTextColor(Colors.textSecondary)
                gravity = Gravity.CENTER
            })
        }

    private fun navCard(title: String, description: String, glyph: String, color: Int, onClick: () -> Unit): View =
        LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(18), dp(16), dp(18), dp(16))
            background = GradientDrawable().apply {
                setColor(Colors.cardBg)
                cornerRadius = dp(8).toFloat()
                setStroke(1, Colors.border)
            }
            elevation = 1f
            layoutParams = LinearLayout.LayoutParams(MATCH, WRAP).apply { setMargins(0, 0, 0, dp(12)) }
            setOnClickListener { onClick() }

            addView(TextView(this@MainActivity).apply {
                text = glyph
                textSize = 17f
                typeface = Typeface.DEFAULT_BOLD
                setTextColor(color)
                gravity = Gravity.CENTER
                background = GradientDrawable().apply {
                    shape = GradientDrawable.OVAL
                    setColor(withAlpha(color, 0.12f))
                }
                layoutParams = LinearLayout.LayoutParams(dp(52), dp(52)).apply { setMargins(0, 0, dp(14), 0) }
            })
            addView(LinearLayout(this@MainActivity).apply {
                orientation = LinearLayout.VERTICAL
                layoutParams = LinearLayout.LayoutParams(0, WRAP, 1f)
                addView(TextView(this@MainActivity).apply {
                    text = title
                    textSize = 17f
                    typeface = Typeface.DEFAULT_BOLD
                    setTextColor(Colors.textPrimary)
                })
                addView(TextView(this@MainActivity).apply {
                    text = description
                    textSize = 12f
                    setTextColor(Colors.textSecondary)
                    lineHeight = dp(19)
                    setPadding(0, dp(4), 0, 0)
                })
            })
            addView(TextView(this@MainActivity).apply {
                text = ">"
                textSize = 20f
                setTextColor(Colors.tabInactive)
                gravity = Gravity.CENTER
            })
        }

    private fun withAlpha(color: Int, factor: Float): Int =
        Color.argb((255 * factor).toInt(), Color.red(color), Color.green(color), Color.blue(color))

    companion object {
        private val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
        private val WRAP = ViewGroup.LayoutParams.WRAP_CONTENT
    }
}
