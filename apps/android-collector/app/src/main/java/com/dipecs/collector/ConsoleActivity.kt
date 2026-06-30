package com.dipecs.collector

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
import android.view.View
import android.view.ViewGroup
import android.widget.AdapterView
import android.widget.ArrayAdapter
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import com.dipecs.collector.actions.ActionExecutorBridge
import com.dipecs.collector.actions.AccessibleContentPrefetcher
import com.dipecs.collector.net.CloudUploader
import com.dipecs.collector.storage.CollectorPreferences
import com.dipecs.collector.storage.EventRepository
import com.dipecs.collector.storage.EventStore
import org.json.JSONObject

class ConsoleActivity : Activity() {

    private lateinit var prefetchTargetInput: EditText
    private lateinit var endpointInput: EditText
    private lateinit var apiKeyInput: EditText
    private lateinit var actionSocketPortInput: EditText
    private lateinit var modeSpinner: Spinner
    private lateinit var uploadEnabledCheck: CheckBox
    private lateinit var debugContainer: LinearLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildPage())
        loadPrefs()
    }

    override fun onResume() {
        super.onResume()
        loadPrefs()
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQ_DOCUMENT || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        val flags = data.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION
        if (flags != 0) runCatching { contentResolver.takePersistableUriPermission(uri, flags) }
        CollectorPreferences.setPrefetchTarget(this, "uri:$uri")
        prefetchTargetInput.setText(CollectorPreferences.prefetchTarget(this))
        EventRepository.recordInternal(this, "prefetch_uri_selected", "URI saved",
            JSONObject().put("target", CollectorPreferences.prefetchTarget(this)))
        toast("已保存 URI 预取目标")
    }

    // ── 构建页面 ──────────────────────────────────────

    private fun buildPage(): View {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Colors.background)
        }
        root.addView(buildAppTopBar("操作控制台"))

        val scroll = ScrollView(this).apply { layoutParams = LinearLayout.LayoutParams(MATCH, 0, 1f) }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(14), dp(16), dp(18))
        }

        content.addView(buildQuickActions())
        content.addView(buildCloudBridge())
        content.addView(buildActionSocket())
        content.addView(buildDataManagement())

        debugContainer = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(debugContainer)
        addAuthorizedActionCard(debugContainer)

        scroll.addView(content)
        root.addView(scroll)
        root.addView(buildBottomNav(AppPage.Console))
        return root
    }

    // ── 快捷动作 ──────────────────────────────────────

    private fun buildQuickActions(): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        prefetchTargetInput = makeInput("url:https://example.test/feed.json  或  uri:content://...")
        content.addView(sectionLabel("预取目标地址"))
        content.addView(prefetchTargetInput)
        content.addView(hintText("支持 url:https:// 远程地址和 uri:content:// 持久化文档。缓存有效期 24 小时。"))

        content.addView(primaryButton("执行预取") {
            val t = prefetchTargetInput.text.toString().trim()
            CollectorPreferences.setPrefetchTarget(this, t)
            ActionExecutorBridge.dispatch(this, ActionExecutorBridge.ACTION_TYPE_PREFETCH_FILE, t, reason = "manual")
            toast("预取已加入队列")
        })
        content.addView(secondaryButton("选择文档 URI") {
            startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE); type = "*/*"
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
            }, REQ_DOCUMENT)
        })
        content.addView(secondaryButton("释放预取缓存") {
            ActionExecutorBridge.dispatch(this, ActionExecutorBridge.ACTION_TYPE_RELEASE_MEMORY, "cache:prefetch", reason = "manual")
            toast("释放缓存已加入队列")
        })
        content.addView(secondaryButton("调度 KeepAlive 任务") {
            ActionExecutorBridge.dispatch(this, ActionExecutorBridge.ACTION_TYPE_KEEP_ALIVE, "work:collector_heartbeat", reason = "manual")
            toast("KeepAlive 已调度")
        })
        content.addView(secondaryButton("预热自有资源") {
            ActionExecutorBridge.dispatch(this, ActionExecutorBridge.ACTION_TYPE_PREWARM_PROCESS, "own:resources", reason = "manual")
            toast("资源预热完成")
        })

        return wrapCard("快捷动作", content)
    }

    // ── 云端桥接 ──────────────────────────────────────

    private fun buildCloudBridge(): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        modeSpinner = Spinner(this).apply {
            adapter = ArrayAdapter(this@ConsoleActivity, android.R.layout.simple_spinner_dropdown_item,
                listOf(CollectorPreferences.MODE_MOCK, CollectorPreferences.MODE_LLM))
            onItemSelectedListener = object : AdapterView.OnItemSelectedListener {
                override fun onItemSelected(p: AdapterView<*>?, v: View?, pos: Int, id: Long) {
                    CollectorPreferences.setUploadMode(this@ConsoleActivity,
                        p?.getItemAtPosition(pos)?.toString() ?: CollectorPreferences.MODE_MOCK)
                }
                override fun onNothingSelected(p: AdapterView<*>?) = Unit
            }
        }
        content.addView(sectionLabel("上传模式"))
        content.addView(modeSpinner)

        uploadEnabledCheck = CheckBox(this).apply {
            text = "启用定时上传"; textSize = 14f
            setTextColor(Colors.textPrimary)
        }
        content.addView(uploadEnabledCheck)

        endpointInput = makeInput("https://example.test/collector")
        content.addView(sectionLabel("端点地址"))
        content.addView(endpointInput)

        apiKeyInput = makeInput("仅 LLM 模式使用"); apiKeyInput.inputType =
            android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD
        content.addView(sectionLabel("LLM API Key"))
        content.addView(apiKeyInput)

        content.addView(primaryButton("保存云端配置") { savePrefs() })
        return wrapCard("云端桥接", content)
    }

    // ── 动作桥接 ──────────────────────────────────────

    private fun buildActionSocket(): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

        actionSocketPortInput = makeInput(CollectorPreferences.DEFAULT_ACTION_SOCKET_PORT.toString())
        actionSocketPortInput.inputType = android.text.InputType.TYPE_CLASS_NUMBER
        actionSocketPortInput.setText(CollectorPreferences.actionSocketPort(this).toString())
        content.addView(sectionLabel("桥接端口"))
        content.addView(actionSocketPortInput)
        content.addView(hintText("宿主机通过 adb forward tcp:PORT tcp:PORT 转发。连接需要 auth_token 和 HMAC-SHA256 签名。"))

        content.addView(secondaryButton("复制认证令牌") { copyToken() })
        return wrapCard("动作桥接", content)
    }

    // ── 数据管理 ──────────────────────────────────────

    private fun buildDataManagement(): View {
        val content = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        content.addView(secondaryButton("立即上传最近事件") {
            savePrefs()
            CloudUploader.uploadRecent(this@ConsoleActivity, reason = "manual")
            toast("上传已加入队列")
        })
        content.addView(secondaryButton("导出 JSONL 追踪") { confirmExport() })
        content.addView(dangerButton("清空本地追踪") { confirmClear() })
        return wrapCard("数据管理", content)
    }

    // ── 辅助方法 ──────────────────────────────────────

    private fun makeInput(hint: String) = EditText(this).apply {
        this.hint = hint
        setSingleLine(true); textSize = 14f
        setPadding(16, 12, 16, 12)
        background = GradientDrawable().apply {
            setColor(Colors.background)
            cornerRadius = 8f
        }
    }

    private fun hintText(text: String) = TextView(this).apply {
        this.text = text; textSize = 11f
        setTextColor(Colors.textSecondary)
        setPadding(0, 4, 0, 8)
    }

    private fun loadPrefs() {
        endpointInput.setText(CollectorPreferences.endpoint(this))
        apiKeyInput.setText(CollectorPreferences.apiKey(this))
        prefetchTargetInput.setText(CollectorPreferences.prefetchTarget(this))
        actionSocketPortInput.setText(CollectorPreferences.actionSocketPort(this).toString())
        uploadEnabledCheck.isChecked = CollectorPreferences.isUploadEnabled(this)
        val mode = CollectorPreferences.uploadMode(this)
        modeSpinner.setSelection(if (mode == CollectorPreferences.MODE_LLM) 1 else 0)
    }

    private fun savePrefs() {
        CollectorPreferences.setEndpoint(this, endpointInput.text.toString())
        CollectorPreferences.setApiKey(this, apiKeyInput.text.toString())
        CollectorPreferences.setUploadEnabled(this, uploadEnabledCheck.isChecked)
        CollectorPreferences.setPrefetchTarget(this, prefetchTargetInput.text.toString())
        val text = actionSocketPortInput.text.toString().trim()
        val port = text.toIntOrNull()
        if (port == null || port !in 1024..65535) {
            toast("端口号必须在 1024 到 65535 之间")
            actionSocketPortInput.setText(CollectorPreferences.actionSocketPort(this).toString())
            return
        }
        CollectorPreferences.setActionSocketPort(this, port)
        EventRepository.recordInternal(this, "upload_config_saved", "Config saved",
            JSONObject().put("mode", CollectorPreferences.uploadMode(this)))
        toast("已保存")
    }

    private fun copyToken() {
        val cm = getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        if (cm == null) { toast("剪贴板不可用"); return }
        val token = CollectorPreferences.actionSocketToken(this)
        val clip = ClipData.newPlainText("DiPECS action socket token", token)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            clip.description.extras = PersistableBundle().apply { putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true) }
        }
        cm.setPrimaryClip(clip)
        toast("认证令牌已复制")
    }

    private fun confirmExport() {
        val stats = EventStore(this).stats()
        AlertDialog.Builder(this)
            .setTitle("导出脱敏追踪？")
            .setMessage("将脱敏后的 JSONL 副本写入外部文件目录。\n总行数: ${stats.totalRows}，rawEvent 行数: ${stats.rawEventRows}。")
            .setPositiveButton("导出") { _, _ ->
                val target = EventStore(this).exportToExternalFiles()
                CollectorPreferences.setLastExport(this, target.absolutePath, System.currentTimeMillis())
                toast("已导出至 ${target.absolutePath}")
            }.setNegativeButton("取消", null).show()
    }

    private fun confirmClear() {
        AlertDialog.Builder(this)
            .setTitle("清空本地追踪？")
            .setMessage("将删除本地 JSONL 追踪和预取缓存。如需保留请先导出。")
            .setPositiveButton("清空") { _, _ ->
                EventStore(this).clear()
                val n = AccessibleContentPrefetcher.clearCache(this)
                toast("已清空；缓存文件已删除: $n")
            }.setNegativeButton("取消", null).show()
    }

    companion object {
        private const val REQ_DOCUMENT = 3302
        private val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
    }
}
